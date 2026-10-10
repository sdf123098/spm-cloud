//! New visual routes use the negotiated byte limit on the wire, including whitespace,
//! and reject duplicate names before Serde can collapse a variable map.
use crate::{api::AppState, error::CloudError};
use axum::extract::{FromRequest, Request};
use serde::de::{DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::{collections::HashSet, fmt};

pub struct VisualJson<T>(pub T);

impl<T: DeserializeOwned + Send> FromRequest<AppState> for VisualJson<T> {
    type Rejection = CloudError;
    async fn from_request(request: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        let content_type = request
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if content_type != "application/json"
            && !(content_type.starts_with("application/") && content_type.ends_with("+json"))
        {
            return Err(CloudError::invalid_metadata(
                "visual metadata requires application/json",
            ));
        }
        let limit = state.runtime_config().visual.limits.max_visual_state_bytes;
        let bytes = axum::body::to_bytes(request.into_body(), limit)
            .await
            .map_err(|_| CloudError::MessageTooLarge)?;
        parse(&bytes, limit).map(Self)
    }
}

fn parse<T: DeserializeOwned>(bytes: &[u8], limit: usize) -> Result<T, CloudError> {
    if bytes.len() > limit {
        return Err(CloudError::MessageTooLarge);
    }
    let mut reader = serde_json::Deserializer::from_slice(bytes);
    Unique(0)
        .deserialize(&mut reader)
        .and_then(|()| reader.end())
        .map_err(|_| CloudError::invalid_metadata("invalid or duplicate visual metadata JSON"))?;
    serde_json::from_slice(bytes)
        .map_err(|_| CloudError::invalid_metadata("invalid visual metadata fields"))
}

struct Unique(usize);
impl<'de> DeserializeSeed<'de> for Unique {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, reader: D) -> Result<(), D::Error> {
        if self.0 > 32 {
            return Err(serde::de::Error::custom("visual JSON nesting exceeded"));
        }
        reader.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Unique {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("JSON with unique member names")
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<(), A::Error> {
        while values.next_element_seed(Unique(self.0 + 1))?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<(), A::Error> {
        let mut names = HashSet::new();
        while let Some(name) = values.next_key::<String>()? {
            if !names.insert(name) {
                return Err(serde::de::Error::custom("duplicate visual JSON name"));
            }
            values.next_value_seed(Unique(self.0 + 1))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_duplicates_inside_variable_maps_escaped_aliases_depth_utf8_and_trailing_data() {
        for bytes in [
            br#"{"variables":{"pose":1,"pose":2}}"#.as_slice(),
            br#"{"variables":{"pose":1,"\u0070ose":2}}"#.as_slice(),
            b"{} {}",
            b"{\"x\":\"\xff\"}",
        ] {
            assert!(parse::<serde_json::Value>(bytes, 8192).is_err());
        }
        let deep = format!("{}0{}", "[".repeat(33), "]".repeat(33));
        assert!(parse::<serde_json::Value>(deep.as_bytes(), 8192).is_err());
        assert!(matches!(
            parse::<serde_json::Value>(b"{}          ", 4),
            Err(CloudError::MessageTooLarge)
        ));
        assert_eq!(
            parse::<serde_json::Value>(br#"{"a":{"pose":1},"b":{"pose":2}}"#, 8192).unwrap()["b"]["pose"],
            2
        );
    }
    #[tokio::test]
    async fn extractor_enforces_wire_limit_before_decoding_including_chunked_bodies() {
        let dir = tempfile::tempdir().unwrap();
        let config =
            crate::config_file::LoadedConfig::load_with(None, dir.path(), &Default::default())
                .unwrap()
                .cloud;
        let store = crate::CloudStore::open(&config).unwrap();
        let state = AppState::new(config, store);
        let mut runtime_config = state.runtime_config();
        runtime_config.visual.limits.max_visual_state_bytes = 4;
        state.update_runtime_config(runtime_config);
        let request = Request::builder()
            .header("content-type", "application/json")
            .body(axum::body::Body::from_stream(futures_util::stream::iter([
                Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"{}")),
                Ok(bytes::Bytes::from_static(b"     ")),
            ])))
            .unwrap();
        assert!(matches!(
            VisualJson::<serde_json::Value>::from_request(request, &state).await,
            Err(CloudError::MessageTooLarge)
        ));
    }
}
