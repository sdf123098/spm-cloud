//! Mojang public trust anchors and the same domain-bound proofs used by the official Worker.
//! No game access token, private key or client-supplied trust anchor is accepted.
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use rsa::{
    RsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
    signature::Verifier,
    traits::PublicKeyParts,
};
use serde_json::Value;
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};
use uuid::Uuid;

const SNAPSHOT: &str = include_str!("../cloudflare/src/mojang-certificate-keys.json");
const ROOT_URL: &str = "https://api.minecraftservices.com/publickeys";

pub fn payload(
    origin: &str,
    purpose: &str,
    account: Option<&str>,
    provider: &str,
    profile: Uuid,
    challenge: &str,
    nonce: &str,
) -> String {
    [
        "SPM-CLOUD-GAME-IDENTITY-V1",
        origin,
        purpose,
        account.unwrap_or(""),
        provider,
        &profile.to_string(),
        challenge,
        nonce,
    ]
    .join("\n")
}

fn decoded(value: &Value, maximum: usize) -> Option<Vec<u8>> {
    let text = value.as_str()?;
    if text.is_empty() || text.len() > maximum {
        return None;
    }
    let bytes = STANDARD.decode(text).ok()?;
    (STANDARD.encode(&bytes) == text).then_some(bytes)
}

fn rsa_key(bytes: &[u8]) -> Option<RsaPublicKey> {
    let key = RsaPublicKey::from_public_key_der(bytes).ok()?;
    (2048..=8192).contains(&key.n().bits()).then_some(key)
}

fn roots(value: &Value, family: &str) -> Option<Vec<RsaPublicKey>> {
    let entries = value[family].as_array()?;
    if entries.is_empty() || entries.len() > 8 {
        return None;
    }
    entries
        .iter()
        .map(|entry| rsa_key(&decoded(&entry["publicKey"], 2048)?))
        .collect()
}

fn signed_sha1(roots: &[RsaPublicKey], data: &[u8], signature: &[u8]) -> bool {
    let Ok(signature) = Signature::try_from(signature) else {
        return false;
    };
    roots.iter().any(|key| {
        VerifyingKey::<sha1::Sha1>::new(key.clone())
            .verify(data, &signature)
            .is_ok()
    })
}

fn verify_key(
    value: &Value,
    profile: Uuid,
    payload: &str,
    now: i64,
    roots: &[RsaPublicKey],
) -> bool {
    let Some(expiry) = value["expires_at_ms"]
        .as_i64()
        .filter(|v| *v > now && *v <= 9_007_199_254_740_991)
    else {
        return false;
    };
    let Some(bytes) = decoded(&value["public_key"], 2048) else {
        return false;
    };
    let Some(key) = rsa_key(&bytes) else {
        return false;
    };
    let Some(key_signature) = decoded(&value["key_signature"], 2048) else {
        return false;
    };
    let Some(challenge_signature) = decoded(&value["challenge_signature"], 2048) else {
        return false;
    };
    let mut certificate = profile.as_bytes().to_vec();
    certificate.extend_from_slice(&expiry.to_be_bytes());
    certificate.extend_from_slice(&bytes);
    if !signed_sha1(roots, &certificate, &key_signature) {
        return false;
    }
    let Ok(signature) = Signature::try_from(challenge_signature.as_slice()) else {
        return false;
    };
    VerifyingKey::<sha2::Sha256>::new(key)
        .verify(payload.as_bytes(), &signature)
        .is_ok()
}

fn verify_name(value: &Value, profile: Uuid, now: i64, roots: &[RsaPublicKey]) -> Option<String> {
    let decoded = decoded(&value["value"], 16384)?;
    let signature = super::profile_proof::decoded(&value["signature"], 2048)?;
    let profile_data: Value = serde_json::from_slice(&decoded).ok()?;
    let id = profile_data["profileId"].as_str()?;
    let name = profile_data["profileName"].as_str()?;
    let timestamp = profile_data["timestamp"].as_i64()?;
    if id.to_ascii_lowercase() != profile.simple().to_string()
        || !valid_name(name)
        || timestamp < now - 300_000
        || timestamp > now + 60_000
    {
        return None;
    }
    signed_sha1(roots, value["value"].as_str()?.as_bytes(), &signature).then(|| name.to_owned())
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 16
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

struct RootCache {
    value: Value,
    retry_at: Instant,
}
static CACHE: OnceLock<tokio::sync::Mutex<RootCache>> = OnceLock::new();

async fn refreshed_roots(family: &str) -> Vec<RsaPublicKey> {
    let cache = CACHE.get_or_init(|| {
        tokio::sync::Mutex::new(RootCache {
            value: serde_json::from_str(SNAPSHOT).expect("embedded Mojang public root snapshot"),
            retry_at: Instant::now(),
        })
    });
    let mut cache = cache.lock().await;
    if Instant::now() >= cache.retry_at {
        cache.retry_at = Instant::now() + Duration::from_secs(300);
        if let Some(value) = fetch_roots().await {
            cache.value = value;
            cache.retry_at = Instant::now() + Duration::from_secs(3600);
        }
    }
    roots(&cache.value, family).unwrap_or_default()
}

async fn fetch_roots() -> Option<Value> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let response = client
        .get(ROOT_URL)
        .header("accept", "application/json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success()
        || !response
            .headers()
            .get("content-type")?
            .to_str()
            .ok()?
            .contains("json")
    {
        return None;
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if bytes.len() + chunk.len() > 32768 {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    roots(&value, "playerCertificateKeys")?;
    roots(&value, "profilePropertyKeys")?;
    Some(value)
}

pub async fn official_key(value: &Value, profile: Uuid, payload: &str) -> bool {
    // Reject malformed/expired certificates before attempting a trust refresh.
    if value["expires_at_ms"]
        .as_i64()
        .is_none_or(|expiry| expiry <= crate::api::now_unix_ms())
        || decoded(&value["public_key"], 2048).is_none()
        || decoded(&value["key_signature"], 2048).is_none()
        || decoded(&value["challenge_signature"], 2048).is_none()
    {
        return false;
    }
    #[cfg(test)]
    if let Some(root) = test_certificate_roots::get(profile) {
        return verify_key(value, profile, payload, crate::api::now_unix_ms(), &[root]);
    }
    let pinned = roots(
        &serde_json::from_str(SNAPSHOT).unwrap(),
        "playerCertificateKeys",
    )
    .unwrap();
    if verify_key(value, profile, payload, crate::api::now_unix_ms(), &pinned) {
        return true;
    }
    verify_key(
        value,
        profile,
        payload,
        crate::api::now_unix_ms(),
        &refreshed_roots("playerCertificateKeys").await,
    )
}

// Test-only trust-fetch seam: the same certificate and challenge verifier still runs.
// No environment, request field, production setting or release symbol can install roots.
#[cfg(test)]
pub(crate) mod test_certificate_roots {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};
    static ROOTS: OnceLock<Mutex<HashMap<Uuid, RsaPublicKey>>> = OnceLock::new();
    pub(crate) struct Guard(Uuid);
    impl Drop for Guard {
        fn drop(&mut self) {
            ROOTS.get().unwrap().lock().unwrap().remove(&self.0);
        }
    }
    pub(crate) fn install(profile: Uuid, encoded: &Value) -> Guard {
        let root = rsa_key(&decoded(encoded, 2048).unwrap()).unwrap();
        ROOTS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert(profile, root);
        Guard(profile)
    }
    pub(super) fn get(profile: Uuid) -> Option<RsaPublicKey> {
        ROOTS
            .get()
            .and_then(|roots| roots.lock().unwrap().get(&profile).cloned())
    }
}

pub async fn official_name(value: &Value, profile: Uuid) -> Option<String> {
    if decoded(&value["value"], 16384).is_none() || decoded(&value["signature"], 2048).is_none() {
        return None;
    }
    let pinned = roots(
        &serde_json::from_str(SNAPSHOT).unwrap(),
        "profilePropertyKeys",
    )
    .unwrap();
    if let Some(name) = verify_name(value, profile, crate::api::now_unix_ms(), &pinned) {
        return Some(name);
    }
    verify_name(
        value,
        profile,
        crate::api::now_unix_ms(),
        &refreshed_roots("profilePropertyKeys").await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Uuid, Vec<RsaPublicKey>, i64) {
        let f: Value =
            serde_json::from_str(include_str!("../tests/fixtures/profile-proofs.json")).unwrap();
        let profile = Uuid::parse_str(f["uuid"].as_str().unwrap()).unwrap();
        let keys = vec![rsa_key(&decoded(&f["root"], 2048).unwrap()).unwrap()];
        let now = f["now"].as_i64().unwrap();
        (f, profile, keys, now)
    }
    #[test]
    fn node_generated_certificate_and_challenge_signatures_match_the_worker_contract() {
        let (f, id, keys, now) = fixture();
        let key = &f["profile_key"];
        let payload = f["payload"].as_str().unwrap();
        assert!(verify_key(key, id, payload, now, &keys));
        assert!(!verify_key(key, Uuid::new_v4(), payload, now, &keys));
        assert!(!verify_key(
            key,
            id,
            &payload.replace("cloud.example.com", "other.example.com"),
            now,
            &keys
        ));
        assert!(!verify_key(
            key,
            id,
            &payload.replace("\nlogin\n", "\nlink\n"),
            now,
            &keys
        ));
        assert!(!verify_key(
            key,
            id,
            payload,
            f["expiry"].as_i64().unwrap(),
            &keys
        ));
        let real = roots(
            &serde_json::from_str(SNAPSHOT).unwrap(),
            "playerCertificateKeys",
        )
        .unwrap();
        assert!(
            !verify_key(key, id, payload, now, &real),
            "a caller's self-signed trust root is never an official root"
        );
        let mut corrupted = key.clone();
        corrupted["key_signature"] = Value::String(STANDARD.encode(vec![0; 256]));
        assert!(!verify_key(&corrupted, id, payload, now, &keys));
    }
    #[test]
    fn signed_canonical_name_checks_uuid_original_base64_and_freshness() {
        let (f, id, keys, now) = fixture();
        let proof = &f["profile_name_proof"];
        assert_eq!(
            verify_name(proof, id, now, &keys).as_deref(),
            Some("Micaftic1")
        );
        assert!(verify_name(proof, Uuid::new_v4(), now, &keys).is_none());
        assert!(verify_name(proof, id, now + 300001, &keys).is_none());
        assert!(verify_name(proof, id, now - 60001, &keys).is_none());
        let mut forged = proof.clone();
        let mut metadata: Value =
            serde_json::from_slice(&decoded(&proof["value"], 16384).unwrap()).unwrap();
        metadata["profileName"] = Value::String("OtherPlayer".into());
        forged["value"] = Value::String(STANDARD.encode(serde_json::to_vec(&metadata).unwrap()));
        assert!(verify_name(&forged, id, now, &keys).is_none());
    }
    #[test]
    fn base64_and_embedded_public_trust_sets_are_bounded_and_valid() {
        assert!(decoded(&Value::String("QQ".into()), 10).is_none());
        assert!(decoded(&Value::String("QQ==\n".into()), 10).is_none());
        assert!(decoded(&Value::String("QUJD".into()), 3).is_none());
        let snapshot: Value = serde_json::from_str(SNAPSHOT).unwrap();
        assert!(roots(&snapshot, "playerCertificateKeys").is_some());
        assert!(roots(&snapshot, "profilePropertyKeys").is_some());
    }
}
