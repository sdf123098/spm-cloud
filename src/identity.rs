use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{config::validate_slug, error::CloudError};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GameIdentity {
    Official { profile_uuid: Uuid },
    Yggdrasil { provider_id: String, profile_uuid: Uuid },
    Offline { scope_id: String, profile_uuid: Uuid },
}

impl GameIdentity {
    pub fn official(profile_uuid: Uuid) -> Self { Self::Official { profile_uuid } }
    pub fn yggdrasil(provider_id: impl Into<String>, profile_uuid: Uuid) -> Result<Self, CloudError> {
        let provider_id = provider_id.into();
        validate_slug(&provider_id, "provider_id")?;
        if provider_id == "official" { return Err(CloudError::invalid_metadata("official is not a Yggdrasil provider")); }
        Ok(Self::Yggdrasil { provider_id, profile_uuid })
    }
    pub fn offline(scope_id: impl Into<String>, profile_uuid: Uuid) -> Result<Self, CloudError> {
        let scope_id = scope_id.into();
        validate_slug(&scope_id, "scope_id")?;
        Ok(Self::Offline { scope_id, profile_uuid })
    }
    pub fn wire_string(&self) -> String {
        match self {
            Self::Official { profile_uuid } => format!("official:{profile_uuid}"),
            Self::Yggdrasil { provider_id, profile_uuid } => format!("yggdrasil:{provider_id}:{profile_uuid}"),
            Self::Offline { scope_id, profile_uuid } => format!("offline:{scope_id}:{profile_uuid}"),
        }
    }
}

impl fmt::Display for GameIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.wire_string()) }
}

impl FromStr for GameIdentity {
    type Err = CloudError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let fields: Vec<&str> = value.split(':').collect();
        match fields.as_slice() {
            ["official", uuid] => Ok(Self::official(parse_uuid(uuid)?)),
            ["yggdrasil", provider_id, uuid] => Self::yggdrasil(*provider_id, parse_uuid(uuid)?),
            ["offline", scope_id, uuid] => Self::offline(*scope_id, parse_uuid(uuid)?),
            _ => Err(CloudError::invalid_metadata("invalid identity reference")),
        }
    }
}

fn parse_uuid(value: &str) -> Result<Uuid, CloudError> {
    Uuid::parse_str(value).map_err(|_| CloudError::invalid_metadata("identity profile_uuid must be a standard UUID"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_with_same_uuid_are_distinct() {
        let uuid = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let refs = [
            GameIdentity::official(uuid),
            GameIdentity::yggdrasil("provider-a", uuid).unwrap(),
            GameIdentity::yggdrasil("provider-b", uuid).unwrap(),
            GameIdentity::offline("scope-a", uuid).unwrap(),
            GameIdentity::offline("scope-b", uuid).unwrap(),
        ];
        let wires: std::collections::HashSet<_> = refs.iter().map(GameIdentity::wire_string).collect();
        assert_eq!(wires.len(), 5);
        for identity in refs { assert_eq!(identity.to_string().parse::<GameIdentity>().unwrap(), identity); }
    }

    #[test]
    fn pseudo_uuid_and_ambiguous_namespaces_are_rejected() {
        assert!("offline:scope:!123e4567-e89b-12d3-a456-426614174000".parse::<GameIdentity>().is_err());
        assert!("official:provider:123e4567-e89b-12d3-a456-426614174000".parse::<GameIdentity>().is_err());
        assert!("yggdrasil:official:123e4567-e89b-12d3-a456-426614174000".parse::<GameIdentity>().is_err());
    }
}

