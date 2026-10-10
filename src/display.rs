//! Player-owned, transport-independent display inputs. None is an unknown value.
use crate::{error::CloudError, visual::VisualSettings};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerDisplayState {
    pub experience_level: Option<u32>,
    pub health: Option<f32>,
    pub max_health: Option<f32>,
    pub food_level: Option<u8>,
    pub effect_amplifiers: Option<BTreeMap<String, u16>>,
    pub flying: Option<bool>,
    pub strafe_input: Option<f32>,
    pub vertical_input: Option<f32>,
    pub forward_input: Option<f32>,
    pub shield_blocking: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlayerDisplayUpdate {
    pub scope_id: String,
    pub world_epoch: String,
    pub dimension_id: String,
    pub state: PlayerDisplayState,
}

pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS player_display_states(identity_id TEXT PRIMARY KEY REFERENCES identities(identity_id) ON DELETE CASCADE, display_json TEXT NOT NULL);")?;
    Ok(())
}

pub(crate) fn context(
    conn: &Connection,
    account: &str,
    scope: &str,
    epoch: &str,
    dimension: &str,
) -> Result<(), CloudError> {
    crate::config::validate_slug(scope, "scope_id")?;
    crate::config::validate_slug(epoch, "world_epoch")?;
    if dimension.len() > 256
        || !dimension.split_once(':').is_some_and(|(namespace, path)| {
            !namespace.is_empty()
                && !path.is_empty()
                && namespace
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
                && path
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
        })
    {
        return Err(CloudError::invalid_metadata("invalid display dimension"));
    }
    let allowed:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM scopes s JOIN scope_acl acl ON acl.scope_id=s.scope_id WHERE s.scope_id=?1 AND s.world_epoch=?2 AND acl.account_id=?3)",params![scope,epoch,account],|r|r.get(0))?;
    if !allowed {
        return Err(CloudError::ScopeAccessDenied);
    }
    Ok(())
}

pub(crate) fn validate(
    value: &PlayerDisplayUpdate,
    settings: &VisualSettings,
) -> Result<(), CloudError> {
    let invalid = || CloudError::invalid_metadata("invalid player display state");
    let s = &value.state;
    if s.experience_level.is_some_and(|v| v > i32::MAX as u32)
        || s.food_level.is_some_and(|v| v > 20)
        || [s.health, s.max_health]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0.0..=1_000_000.0).contains(&v))
        || [s.strafe_input, s.vertical_input, s.forward_input]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || !(-1.0..=1.0).contains(&v))
        || s.effect_amplifiers.as_ref().is_some_and(|values| {
            values.len() > settings.limits.max_visual_variables
                || values.iter().any(|(key, amp)| {
                    key.len() > 256
                        || !key.split_once(':').is_some_and(|(namespace, path)| {
                            !namespace.is_empty()
                                && !path.is_empty()
                                && namespace.bytes().all(|b| {
                                    b.is_ascii_lowercase()
                                        || b.is_ascii_digit()
                                        || b"_.-".contains(&b)
                                })
                                && path.bytes().all(|b| {
                                    b.is_ascii_lowercase()
                                        || b.is_ascii_digit()
                                        || b"_./-".contains(&b)
                                })
                        })
                        || !(1..=256).contains(amp)
                })
        })
    {
        return Err(invalid());
    }
    if serde_json::to_vec(value).map_err(|_| invalid())?.len()
        > settings.limits.max_visual_state_bytes
    {
        return Err(CloudError::MessageTooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_fixture_preserves_unknowns_and_rejects_invalid_inputs() {
        let mut value: PlayerDisplayUpdate =
            serde_json::from_str(include_str!("../fixtures/player-display-state-v1.json")).unwrap();
        let settings = VisualSettings::default();
        validate(&value, &settings).unwrap();
        assert_eq!(
            value.state.effect_amplifiers.as_ref().unwrap()["minecraft:speed"],
            2
        );
        assert_eq!(PlayerDisplayState::default().experience_level, None);
        value.state.forward_input = Some(f32::INFINITY);
        assert!(validate(&value, &settings).is_err());
        value.state.forward_input = Some(1.0);
        value.state.food_level = Some(21);
        assert!(validate(&value, &settings).is_err());
        value.state.food_level = Some(17);
        let mut zero = settings.clone();
        zero.limits.max_visual_variables = 0;
        assert!(validate(&value, &zero).is_err());
        value.state.effect_amplifiers = Some(Default::default());
        validate(&value, &zero).unwrap();
        assert!(
            serde_json::from_value::<PlayerDisplayState>(serde_json::json!({"food_level":1.5}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<PlayerDisplayState>(
                serde_json::json!({"server_command":"op me"})
            )
            .is_err()
        );
    }
    #[test]
    fn verified_self_publication_is_scoped_and_cleared_by_legacy_and_privacy_updates() {
        let dir = tempfile::tempdir().unwrap();
        let mut config =
            crate::config_file::LoadedConfig::load_with(None, dir.path(), &Default::default())
                .unwrap()
                .cloud;
        config.visual.features.player_display_state = true;
        let store = crate::CloudStore::open(&config).unwrap();
        let profile = uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
        store.connection.lock().unwrap().execute_batch("INSERT INTO accounts(account_id,created_at) VALUES ('observer','now'),('outsider','now');
        INSERT INTO scopes(scope_id,tenant_id,name,world_epoch,created_at) VALUES ('test-scope','account_local','Test','world-reset-3','now');
        INSERT INTO scope_acl(scope_id,account_id,role) VALUES ('test-scope','account_local','viewer'),('test-scope','observer','viewer');
        INSERT INTO identities(identity_id,account_id,identity_kind,profile_uuid,display_name,verified) VALUES ('display-self','account_local','official','22222222-2222-4222-8222-222222222222','Self',1);
        INSERT INTO assets(asset_id,owner_account_id,current_revision,visibility) VALUES ('display-model','account_local',1,'PUBLIC');
        INSERT INTO asset_revisions(asset_id,revision,name,format,raw_sha256,byte_length,object_path,created_at) VALUES ('display-model',1,'Model','ysm','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1,'test','now');").unwrap();
        let display: PlayerDisplayUpdate =
            serde_json::from_str(include_str!("../fixtures/player-display-state-v1.json")).unwrap();
        let mut input:crate::player::PlayerAppearanceUpdate=serde_json::from_value(serde_json::json!({"identity_id":"display-self","entity_uuid":profile,"expected_revision":0,"asset_id":"display-model","asset_revision":1,"raw_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","texture_id":"default","display_state":display})).unwrap();
        store.publish_player("account_local", &input).unwrap();
        let read = |account: &str| {
            serde_json::to_value(store.query_players(account, &[profile]).unwrap()).unwrap()
        };
        assert_eq!(
            read("observer")[0]["selection"]["display_state"]["state"]["experience_level"],
            42
        );
        assert!(read("outsider")[0]["selection"]["display_state"].is_null());
        assert!(matches!(
            store.publish_player("observer", &input),
            Err(CloudError::IdentityNotVerified)
        ));
        input.entity_uuid = uuid::Uuid::new_v4();
        assert!(matches!(
            store.publish_player("account_local", &input),
            Err(CloudError::IdentityProfileMismatch)
        ));
        input.entity_uuid = profile;
        input.expected_revision = 1;
        input.display_state = None;
        store.publish_player("account_local", &input).unwrap();
        assert!(read("observer")[0]["selection"]["display_state"].is_null());
        input.expected_revision = 2;
        input.display_state = Some(display);
        store.publish_player("account_local", &input).unwrap();
        input.expected_revision = 3;
        input.asset_id = None;
        input.display_state = None;
        store.publish_player("account_local", &input).unwrap();
        assert!(read("observer")[0]["selection"].is_null());
    }
}
