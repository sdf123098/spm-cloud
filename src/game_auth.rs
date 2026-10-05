//! Passwordless game login and account linking, isolated from the legacy challenge schema.
use crate::{
    CloudStore,
    api::{AppState, authenticate},
    error::CloudError,
    profile_proof,
};
use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
};
use futures_util::StreamExt;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, time::Duration};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct ChallengeInput {
    provider_id: String,
    username: String,
    profile_uuid: Uuid,
}
#[derive(Deserialize)]
pub struct CompleteInput {
    challenge_id: Option<String>,
    profile_key: Option<Value>,
}
struct Challenge {
    provider: String,
    username: String,
    profile: Uuid,
    nonce: String,
}
fn hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}
fn now() -> i64 {
    crate::api::now_unix_ms() / 1000
}

pub fn migrate(conn: &rusqlite::Connection) -> Result<(), CloudError> {
    // Preserve pre-upgrade link challenges and all existing accounts/assets.
    conn.execute_batch("CREATE TABLE IF NOT EXISTS game_auth_challenges (
        challenge_hash TEXT PRIMARY KEY, purpose TEXT NOT NULL CHECK(purpose IN ('link','login')),
        account_id TEXT REFERENCES accounts(account_id), provider_id TEXT NOT NULL REFERENCES identity_providers(provider_id),
        username TEXT NOT NULL, profile_uuid TEXT NOT NULL, server_id TEXT NOT NULL,
        requester_hash TEXT NOT NULL, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, consumed INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX IF NOT EXISTS game_auth_rate ON game_auth_challenges(requester_hash,created_at);")?;
    let has_path: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('identity_providers') WHERE name='session_path')", [], |r| r.get(0))?;
    if !has_path {
        conn.execute_batch("ALTER TABLE identity_providers ADD COLUMN session_path TEXT NOT NULL DEFAULT '/session/minecraft/hasJoined';")?;
    }
    for (id, name, base, path) in [
        (
            "official",
            "Minecraft official",
            "https://sessionserver.mojang.com",
            "/session/minecraft/hasJoined",
        ),
        (
            "littleskin",
            "LittleSkin",
            "https://littleskin.cn/api/yggdrasil",
            "/sessionserver/session/minecraft/hasJoined",
        ),
        (
            "elyby",
            "Ely.by",
            "https://authserver.ely.by",
            "/session/hasJoined",
        ),
        (
            "drasl_unmojang",
            "Drasl (unmojang.org)",
            "https://drasl.unmojang.org",
            "/session/minecraft/hasJoined",
        ),
    ] {
        conn.execute("INSERT OR IGNORE INTO identity_providers(provider_id,display_name,base_url,session_path,enabled) VALUES (?1,?2,?3,?4,1)", params![id,name,base,path])?;
    }
    Ok(())
}

pub async fn login_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Json(input): Json<ChallengeInput>,
) -> Result<Json<Value>, CloudError> {
    create(
        &state,
        "login",
        None,
        &input,
        &requester(&state, &headers, peer.map(|p| p.0.0.ip())),
    )
}
pub async fn link_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Json(input): Json<ChallengeInput>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    create(
        &state,
        "link",
        Some(&account),
        &input,
        &requester(&state, &headers, peer.map(|p| p.0.0.ip())),
    )
}

fn requester(state: &AppState, headers: &HeaderMap, peer: Option<std::net::IpAddr>) -> String {
    if peer.is_some_and(|ip| state.config.trusted_proxy_ips.contains(&ip)) {
        if let Some(ip) = headers
            .get("x-spm-client-ip")
            .and_then(|h| h.to_str().ok())
            .and_then(|v| v.parse::<std::net::IpAddr>().ok())
        {
            return ip.to_string();
        }
    }
    peer.map(|p| p.to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn create(
    state: &AppState,
    purpose: &str,
    account: Option<&str>,
    input: &ChallengeInput,
    requester: &str,
) -> Result<Json<Value>, CloudError> {
    crate::config::validate_slug(&input.provider_id, "provider_id")?;
    if input.username.is_empty()
        || input.username.encode_utf16().count() > 64
        || input.username.chars().any(char::is_control)
    {
        return Err(CloudError::invalid_metadata("invalid username"));
    }
    let conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let enabled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM identity_providers WHERE provider_id=?1 AND enabled=1)",
        [&input.provider_id],
        |r| r.get(0),
    )?;
    if !enabled {
        return Err(CloudError::IdentityProviderUntrusted);
    }
    let requester = hash(requester);
    let recent: i64 = conn.query_row(
        "SELECT COUNT(*) FROM game_auth_challenges WHERE requester_hash=?1 AND created_at>?2",
        params![requester, now() - 120],
        |r| r.get(0),
    )?;
    if recent >= 20 {
        return Err(CloudError::RateLimited);
    }
    conn.execute(
        "DELETE FROM game_auth_challenges WHERE expires_at<?1",
        [now() - 300],
    )?;
    let id = format!("challenge_{}", Uuid::new_v4().simple());
    let nonce = Uuid::new_v4().simple().to_string();
    conn.execute("INSERT INTO game_auth_challenges(challenge_hash,purpose,account_id,provider_id,username,profile_uuid,server_id,requester_hash,created_at,expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![hash(&id),purpose,account,input.provider_id,input.username,input.profile_uuid.to_string(),nonce,requester,now(),now()+120])?;
    let mut response = json!({"challenge_id":id,"provider_id":input.provider_id,"server_id":nonce,"expires_in_seconds":120});
    if input.provider_id == "official" {
        response["profile_key_payload"] = json!(profile_proof::payload(
            &state.config.origin,
            purpose,
            account,
            &input.provider_id,
            input.profile_uuid,
            &id,
            &nonce
        ));
    }
    Ok(Json(response))
}

pub async fn login_complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CompleteInput>,
) -> Result<Json<Value>, CloudError> {
    complete(&state, "login", None, &id, &input).await
}
pub async fn link_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<CompleteInput>,
) -> Result<Json<Value>, CloudError> {
    let account = authenticate(&state, &headers)?;
    complete(&state, "link", Some(&account), &id, &input).await
}

fn claim(
    state: &AppState,
    purpose: &str,
    account: Option<&str>,
    id: &str,
) -> Result<Challenge, CloudError> {
    let mut conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let tx = conn.transaction()?;
    let row: Option<(String,String,String,String,i64,i64)> = tx.query_row("SELECT provider_id,username,profile_uuid,server_id,expires_at,consumed FROM game_auth_challenges WHERE challenge_hash=?1 AND purpose=?2 AND account_id IS ?3",params![hash(id),purpose,account],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let (provider, username, profile, nonce, expires, consumed) =
        row.ok_or(CloudError::IdentityChallengeExpired)?;
    if consumed != 0 {
        return Err(CloudError::IdentityChallengeReplayed);
    }
    if expires <= now() {
        return Err(CloudError::IdentityChallengeExpired);
    }
    tx.execute(
        "UPDATE game_auth_challenges SET consumed=1 WHERE challenge_hash=?1",
        [hash(id)],
    )?;
    tx.commit()?;
    Ok(Challenge {
        provider,
        username,
        profile: Uuid::parse_str(&profile).map_err(|_| CloudError::IdentityProfileMismatch)?,
        nonce,
    })
}

async fn complete(
    state: &AppState,
    purpose: &str,
    account: Option<&str>,
    id: &str,
    input: &CompleteInput,
) -> Result<Json<Value>, CloudError> {
    if input.challenge_id.as_deref() != Some(id) {
        return Err(CloudError::invalid_metadata(
            "challenge_id path/body mismatch",
        ));
    }
    // Consume before network I/O or crypto; failed proofs cannot be retried against the same nonce.
    let challenge = claim(state, purpose, account, id)?;
    let (base, path) = provider(&state.store, &challenge.provider)?;
    let name = if let Some(key) = &input.profile_key {
        if challenge.provider != "official"
            || !profile_proof::official_key(
                key,
                challenge.profile,
                &profile_proof::payload(
                    &state.config.origin,
                    purpose,
                    account,
                    &challenge.provider,
                    challenge.profile,
                    id,
                    &challenge.nonce,
                ),
            )
            .await
        {
            return Err(CloudError::IdentityProfileMismatch);
        }
        None // A certificate proves UUID ownership, never a canonical name.
    } else {
        Some(session_name(&challenge, &base, &path).await?)
    };
    Ok(Json(state.store.finish_game_identity(
        purpose,
        account,
        &challenge.provider,
        challenge.profile,
        &challenge.username,
        name.as_deref(),
    )?))
}

fn provider(store: &CloudStore, id: &str) -> Result<(String, String), CloudError> {
    let conn = store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    conn.query_row(
        "SELECT base_url,session_path FROM identity_providers WHERE provider_id=?1 AND enabled=1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?
    .ok_or(CloudError::IdentityProviderUntrusted)
}

pub fn session_path(path: &str) -> Result<(), CloudError> {
    if matches!(
        path,
        "/sessionserver/session/minecraft/hasJoined"
            | "/session/minecraft/hasJoined"
            | "/session/hasJoined"
    ) {
        Ok(())
    } else {
        Err(CloudError::IdentityProviderUntrusted)
    }
}
pub fn provider_url(base: &str) -> Result<reqwest::Url, CloudError> {
    let url = reqwest::Url::parse(base).map_err(|_| CloudError::IdentityProviderUntrusted)?;
    let host = url
        .host_str()
        .ok_or(CloudError::IdentityProviderUntrusted)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !host.contains('.')
        || host.parse::<std::net::IpAddr>().is_ok()
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err(CloudError::IdentityProviderUntrusted);
    }
    Ok(url)
}

async fn session_name(challenge: &Challenge, base: &str, path: &str) -> Result<String, CloudError> {
    session_path(path)?;
    let mut url = provider_url(base)?;
    let host = url.host_str().unwrap().to_owned();
    let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 443))
        .await
        .map_err(|_| CloudError::IdentityProviderUnavailable)?
        .collect();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|a| !crate::api::is_allowed_provider_address(a.ip()))
    {
        return Err(CloudError::IdentityProviderUntrusted);
    }
    url.set_path(&format!("{}{}", url.path().trim_end_matches('/'), path));
    url.query_pairs_mut()
        .append_pair("username", &challenge.username)
        .append_pair("serverId", &challenge.nonce);
    let client = reqwest::Client::builder()
        .resolve_to_addrs(&host, &addresses)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| CloudError::IdentityProviderUnavailable)?;
    let response = client
        .get(url)
        .header("accept", "application/json")
        .header("user-agent", "SPM-Cloud/1.0")
        .send()
        .await
        .map_err(|_| CloudError::IdentityProviderUnavailable)?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| CloudError::IdentityProviderUnavailable)?;
        if bytes.len() + chunk.len() > 65536 {
            return Err(CloudError::IdentityProviderUnavailable);
        }
        bytes.extend_from_slice(&chunk);
    }
    session_response(challenge, path, status, &content_type, &bytes)
}

fn session_response(
    challenge: &Challenge,
    path: &str,
    status: u16,
    content_type: &str,
    bytes: &[u8],
) -> Result<String, CloudError> {
    if status == 204
        || status == 404
        || (challenge.provider != "official"
            && path == "/session/minecraft/hasJoined"
            && status == 403
            && content_type.is_empty()
            && bytes.iter().all(u8::is_ascii_whitespace))
    {
        return Err(CloudError::IdentityProfileMismatch);
    }
    let parsed: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
    if path == "/session/hasJoined"
        && status == 401
        && content_type.contains("json")
        && parsed["error"] == "ForbiddenOperationException"
        && parsed["errorMessage"] == "Invalid token."
    {
        return Err(CloudError::IdentityProfileMismatch);
    }
    if !(200..300).contains(&status) || !content_type.contains("json") || !parsed.is_object() {
        return Err(CloudError::IdentityProviderUnavailable);
    }
    let uuid = parsed["id"].as_str().and_then(|v| Uuid::parse_str(v).ok());
    let name = parsed["name"].as_str().filter(|v| {
        !v.is_empty()
            && v.chars().count() <= 64
            && !v.chars().any(char::is_control)
            && v.eq_ignore_ascii_case(&challenge.username)
    });
    if uuid != Some(challenge.profile) || name.is_none() {
        return Err(CloudError::IdentityProfileMismatch);
    }
    Ok(name.unwrap().to_owned())
}

impl CloudStore {
    pub(crate) fn finish_game_identity(
        &self,
        purpose: &str,
        account: Option<&str>,
        provider: &str,
        profile: Uuid,
        display: &str,
        canonical_name: Option<&str>,
    ) -> Result<Value, CloudError> {
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let tx = conn.transaction()?;
        let kind = if provider == "official" {
            "official"
        } else {
            "yggdrasil"
        };
        let provider_id = (provider != "official").then_some(provider);
        let existing: Vec<(String, String)> = {
            let mut query = tx.prepare("SELECT identity_id,account_id FROM identities WHERE identity_kind=?1 AND COALESCE(provider_id,'')=COALESCE(?2,'') AND profile_uuid=?3 AND verified=1 ORDER BY identity_id")?;
            query
                .query_map(params![kind, provider_id, profile.to_string()], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        if purpose == "login" {
            let owners: std::collections::HashSet<_> =
                existing.iter().map(|(_, owner)| owner.clone()).collect();
            if owners.len() != 1 {
                return Err(CloudError::IdentityNotLinked);
            }
            let owner = owners.into_iter().next().unwrap();
            if let Some(name) = canonical_name {
                for (id, _) in &existing {
                    tx.execute("INSERT OR REPLACE INTO player_canonical_names(identity_id,name) VALUES (?1,?2)",params![id,name])?;
                    tx.execute(
                        "UPDATE identities SET display_name=?1 WHERE identity_id=?2",
                        params![name, id],
                    )?;
                }
            }
            tx.commit()?;
            let session = crate::store::issue_session_in_transaction(&mut conn, &owner)?;
            let mut response =
                serde_json::to_value(session).map_err(|e| CloudError::Internal(e.into()))?;
            response["account_id"] = json!(owner);
            return Ok(response);
        }
        let account = account.ok_or(CloudError::Unauthenticated)?;
        if existing.iter().any(|(_, owner)| owner != account) {
            return Err(CloudError::IdentityAlreadyLinked);
        }
        let id = if let Some((id, _)) = existing.first() {
            id.clone()
        } else {
            // Reuse this account's provisional identity instead of creating a conflicting reservation.
            tx.query_row("SELECT identity_id FROM identities WHERE account_id=?1 AND identity_kind=?2 AND COALESCE(provider_id,'')=COALESCE(?3,'') AND profile_uuid=?4 AND verified=0 LIMIT 1",params![account,kind,provider_id,profile.to_string()],|r|r.get(0)).optional()?.unwrap_or_else(||format!("identity_{}",Uuid::new_v4().simple()))
        };
        tx.execute("INSERT INTO identities(identity_id,account_id,identity_kind,provider_id,scope_id,profile_uuid,display_name,verified) VALUES (?1,?2,?3,?4,NULL,?5,?6,1) ON CONFLICT(identity_id) DO UPDATE SET verified=1,display_name=excluded.display_name",params![id,account,kind,provider_id,profile.to_string(),canonical_name.unwrap_or(display)])?;
        if let Some(name) = canonical_name {
            tx.execute(
                "INSERT OR REPLACE INTO player_canonical_names(identity_id,name) VALUES (?1,?2)",
                params![id, name],
            )?;
        }
        tx.commit()?;
        Ok(
            json!({"identity_id":id,"account_id":account,"identity":if kind=="official" {format!("official:{profile}")} else {format!("yggdrasil:{provider}:{profile}")},"display_name":canonical_name.unwrap_or(display),"verification_status":"VERIFIED"}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::tests::test_state;
    #[tokio::test]
    async fn http_game_registration_link_login_proofs_and_ownership() {
        use std::io::Write;
        let (state, _dir) = test_state(true);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, crate::api::router(state))
                .await
                .unwrap()
        });
        let client = reqwest::Client::new();
        let base = format!("http://{address}");
        async fn post(
            client: &reqwest::Client,
            base: &str,
            path: &str,
            body: Value,
            token: Option<&str>,
            status: u16,
        ) -> Value {
            let mut request = client.post(format!("{base}{path}")).json(&body);
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            let response = request.send().await.unwrap();
            assert_eq!(response.status().as_u16(), status, "{path}");
            response.json().await.unwrap()
        }
        fn proof(
            profile: Uuid,
            challenge: &Value,
        ) -> (Value, crate::profile_proof::test_certificate_roots::Guard) {
            let mut command = std::process::Command::new("node");
            command
                .arg(format!(
                    "{}/tools/gameauth_test_proof.mjs",
                    env!("CARGO_MANIFEST_DIR")
                ))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped());
            let mut process = command.spawn().unwrap();
            process
                .stdin
                .take()
                .unwrap()
                .write_all(
                    json!({"profile":profile,"payload":challenge["profile_key_payload"]})
                        .to_string()
                        .as_bytes(),
                )
                .unwrap();
            let output = process.wait_with_output().unwrap();
            assert!(output.status.success());
            let fixture: Value = serde_json::from_slice(&output.stdout).unwrap();
            let guard =
                crate::profile_proof::test_certificate_roots::install(profile, &fixture["root"]);
            (fixture["profile_key"].clone(), guard)
        }
        let mut tokens = Vec::new();
        for account in ["http-owner", "http-other"] {
            post(
                &client,
                &base,
                "/v1/accounts",
                json!({"account_id":account,"password":"http-test-password"}),
                None,
                201,
            )
            .await;
            let session = post(
                &client,
                &base,
                "/v1/sessions",
                json!({"account_id":account,"password":"http-test-password"}),
                None,
                200,
            )
            .await;
            tokens.push(session["access_token"].as_str().unwrap().to_owned());
        }
        let profile = Uuid::new_v4();
        let input = json!({"provider_id":"official","username":"Player","profile_uuid":profile});
        // A valid certificate alone cannot log in an identity that is not linked.
        let challenge = post(
            &client,
            &base,
            "/v1/auth/login-challenges",
            input.clone(),
            None,
            200,
        )
        .await;
        let id = challenge["challenge_id"].as_str().unwrap();
        let (key, guard) = proof(profile, &challenge);
        let result = post(
            &client,
            &base,
            &format!("/v1/auth/login-challenges/{id}/complete"),
            json!({"challenge_id":id,"profile_key":key}),
            None,
            404,
        )
        .await;
        assert_eq!(result["code"], "IDENTITY_NOT_LINKED");
        drop(guard);
        let challenge = post(
            &client,
            &base,
            "/v1/auth/challenges",
            input.clone(),
            Some(&tokens[0]),
            200,
        )
        .await;
        let id = challenge["challenge_id"].as_str().unwrap();
        let (key, guard) = proof(profile, &challenge);
        let body = json!({"challenge_id":id,"profile_key":key});
        let linked = post(
            &client,
            &base,
            &format!("/v1/auth/challenges/{id}/complete"),
            body.clone(),
            Some(&tokens[0]),
            200,
        )
        .await;
        assert_eq!(linked["verification_status"], "VERIFIED");
        let replay = post(
            &client,
            &base,
            &format!("/v1/auth/challenges/{id}/complete"),
            body,
            Some(&tokens[0]),
            401,
        )
        .await;
        assert_eq!(replay["code"], "IDENTITY_CHALLENGE_REPLAYED");
        drop(guard);
        // The same proven UUID cannot move to another Cloud account.
        let challenge = post(
            &client,
            &base,
            "/v1/auth/challenges",
            input.clone(),
            Some(&tokens[1]),
            200,
        )
        .await;
        let id = challenge["challenge_id"].as_str().unwrap();
        let (key, guard) = proof(profile, &challenge);
        let refused = post(
            &client,
            &base,
            &format!("/v1/auth/challenges/{id}/complete"),
            json!({"challenge_id":id,"profile_key":key}),
            Some(&tokens[1]),
            409,
        )
        .await;
        assert_eq!(refused["code"], "IDENTITY_ALREADY_LINKED");
        drop(guard);
        let challenge = post(
            &client,
            &base,
            "/v1/auth/login-challenges",
            input.clone(),
            None,
            200,
        )
        .await;
        let id = challenge["challenge_id"].as_str().unwrap();
        let (key, guard) = proof(profile, &challenge);
        let logged_in = post(
            &client,
            &base,
            &format!("/v1/auth/login-challenges/{id}/complete"),
            json!({"challenge_id":id,"profile_key":key}),
            None,
            200,
        )
        .await;
        assert_eq!(logged_in["account_id"], "http-owner");
        let identities: Value = client
            .get(format!("{base}/v1/identities"))
            .bearer_auth(logged_in["access_token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(identities.as_array().unwrap().len(), 1);
        assert_eq!(identities[0]["identity_id"], linked["identity_id"]);
        drop(guard);
        // Keep the real payload binding: signing another domain must fail cryptography.
        let mut challenge = post(
            &client,
            &base,
            "/v1/auth/login-challenges",
            input,
            None,
            200,
        )
        .await;
        challenge["profile_key_payload"] = json!(
            challenge["profile_key_payload"]
                .as_str()
                .unwrap()
                .replace("https://localhost", "https://wrong.example")
        );
        let id = challenge["challenge_id"].as_str().unwrap();
        let (key, guard) = proof(profile, &challenge);
        let invalid = post(
            &client,
            &base,
            &format!("/v1/auth/login-challenges/{id}/complete"),
            json!({"challenge_id":id,"profile_key":key}),
            None,
            403,
        )
        .await;
        assert_eq!(invalid["code"], "IDENTITY_PROFILE_MISMATCH");
        drop(guard);
        server.abort();
    }
    #[tokio::test]
    async fn completion_requires_matching_body_challenge_id_without_consuming_nonce() {
        let (state, _dir) = test_state(false);
        let input = ChallengeInput {
            provider_id: "official".into(),
            username: "Player".into(),
            profile_uuid: Uuid::new_v4(),
        };
        let challenge = create(&state, "login", None, &input, "test-client")
            .unwrap()
            .0;
        let id = challenge["challenge_id"].as_str().unwrap();
        let result = complete(
            &state,
            "login",
            None,
            id,
            &CompleteInput {
                challenge_id: None,
                profile_key: Some(json!({})),
            },
        )
        .await;
        assert!(matches!(result, Err(CloudError::InvalidMetadata(_))));
        assert!(
            claim(&state, "login", None, id).is_ok(),
            "malformed requests must not consume a valid challenge"
        );
    }
    #[test]
    fn challenge_usernames_use_worker_utf16_length_limit() {
        let (state, _dir) = test_state(false);
        let input = ChallengeInput {
            provider_id: "official".into(),
            username: "🌊".repeat(33),
            profile_uuid: Uuid::new_v4(),
        };
        assert!(matches!(
            create(&state, "login", None, &input, "test-client"),
            Err(CloudError::InvalidMetadata(_))
        ));
    }
    #[test]
    fn forwarded_client_ips_are_trusted_only_from_the_configured_proxy() {
        let (mut state, _dir) = test_state(false);
        std::sync::Arc::make_mut(&mut state.config).trusted_proxy_ips =
            vec!["127.0.0.1".parse().unwrap()];
        let mut headers = HeaderMap::new();
        headers.insert("x-spm-client-ip", "1.2.3.4".parse().unwrap());
        assert_eq!(
            requester(&state, &headers, Some("127.0.0.1".parse().unwrap())),
            "1.2.3.4"
        );
        assert_eq!(
            requester(&state, &headers, Some("8.8.8.8".parse().unwrap())),
            "8.8.8.8"
        );
        assert_eq!(requester(&state, &headers, None), "unknown");
        for address in [
            "127.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "10.1.2.3",
            "100.64.0.1",
            "fe80::1",
            "ff02::1",
            "224.0.0.1",
        ] {
            assert!(
                !crate::api::is_allowed_provider_address(address.parse().unwrap()),
                "{address}"
            );
        }
        assert!(crate::api::is_allowed_provider_address(
            "8.8.8.8".parse().unwrap()
        ));
    }
    #[test]
    fn linking_is_idempotent_login_issues_rotatable_sessions_and_cannot_steal_profiles() {
        let (state, _dir) = test_state(false);
        let store = &state.store;
        let profile = Uuid::new_v4();
        assert!(matches!(
            store.finish_game_identity("login", None, "official", profile, "Player", None),
            Err(CloudError::IdentityNotLinked)
        ));
        let original = store
            .finish_game_identity(
                "link",
                Some("account_local"),
                "official",
                profile,
                "UnprovenDisplay",
                None,
            )
            .unwrap();
        let repeated = store
            .finish_game_identity(
                "link",
                Some("account_local"),
                "official",
                profile,
                "UnprovenDisplay",
                None,
            )
            .unwrap();
        assert_eq!(original["identity_id"], repeated["identity_id"]);
        let conn = store.connection.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM player_canonical_names", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "certificate-only display names are never authorized as offline aliases"
        );
        drop(conn);
        store
            .create_account("another", "another-test-password")
            .unwrap();
        assert!(matches!(
            store.finish_game_identity(
                "link",
                Some("another"),
                "official",
                profile,
                "Player",
                Some("Player")
            ),
            Err(CloudError::IdentityAlreadyLinked)
        ));
        let login = store
            .finish_game_identity("login", None, "official", profile, "Player", Some("Player"))
            .unwrap();
        assert_eq!(login["account_id"], "account_local");
        assert_eq!(
            store
                .authenticate_access_token(login["access_token"].as_str().unwrap())
                .unwrap(),
            "account_local"
        );
        let refresh = login["refresh_token"].as_str().unwrap();
        store.refresh_session(refresh).unwrap();
        assert!(matches!(
            store.refresh_session(refresh),
            Err(CloudError::RefreshReused)
        ));
        let other = store
            .finish_game_identity(
                "link",
                Some("another"),
                "elyby",
                profile,
                "Other",
                Some("Other"),
            )
            .unwrap();
        assert_ne!(
            original["identity_id"], other["identity_id"],
            "provider namespaces stay separate"
        );
    }
    #[test]
    fn challenge_rate_purpose_owner_expiry_and_single_use_are_enforced() {
        let (state, _dir) = test_state(false);
        let input = ChallengeInput {
            provider_id: "official".into(),
            username: "Player".into(),
            profile_uuid: Uuid::new_v4(),
        };
        let first = create(&state, "link", Some("account_local"), &input, "client")
            .unwrap()
            .0;
        let id = first["challenge_id"].as_str().unwrap();
        assert!(matches!(
            claim(&state, "login", None, id),
            Err(CloudError::IdentityChallengeExpired)
        ));
        assert!(matches!(
            claim(&state, "link", Some("other"), id),
            Err(CloudError::IdentityChallengeExpired)
        ));
        claim(&state, "link", Some("account_local"), id).unwrap();
        assert!(matches!(
            claim(&state, "link", Some("account_local"), id),
            Err(CloudError::IdentityChallengeReplayed)
        ));
        for _ in 1..20 {
            let _ = create(&state, "login", None, &input, "client").unwrap();
        }
        assert!(matches!(
            create(&state, "login", None, &input, "client"),
            Err(CloudError::RateLimited)
        ));
        let expired = create(&state, "login", None, &input, "other-client")
            .unwrap()
            .0;
        state
            .store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE game_auth_challenges SET expires_at=0 WHERE challenge_hash=?1",
                [hash(expired["challenge_id"].as_str().unwrap())],
            )
            .unwrap();
        assert!(matches!(
            claim(
                &state,
                "login",
                None,
                expired["challenge_id"].as_str().unwrap()
            ),
            Err(CloudError::IdentityChallengeExpired)
        ));
    }
    #[test]
    fn provider_paths_and_http_failures_do_not_claim_unverified_sessions() {
        for path in [
            "/sessionserver/session/minecraft/hasJoined",
            "/session/minecraft/hasJoined",
            "/session/hasJoined",
        ] {
            assert!(session_path(path).is_ok());
        }
        for base in [
            "http://example.com",
            "https://localhost",
            "https://127.0.0.1",
            "https://[::1]",
            "https://example.com:8443",
            "https://secret@example.com",
            "https://a.local",
            "https://a.example.com?token=x",
        ] {
            assert!(provider_url(base).is_err(), "{base}");
        }
        assert!(provider_url("https://littleskin.cn/api/yggdrasil").is_ok());
        let mut c = Challenge {
            provider: "official".into(),
            username: "Player".into(),
            profile: Uuid::new_v4(),
            nonce: "n".into(),
        };
        assert!(matches!(
            session_response(&c, "/session/minecraft/hasJoined", 204, "", b""),
            Err(CloudError::IdentityProfileMismatch)
        ));
        assert!(matches!(
            session_response(
                &c,
                "/session/minecraft/hasJoined",
                403,
                "text/html",
                b"blocked"
            ),
            Err(CloudError::IdentityProviderUnavailable)
        ));
        assert!(matches!(
            session_response(
                &c,
                "/session/minecraft/hasJoined",
                200,
                "application/json",
                b"broken"
            ),
            Err(CloudError::IdentityProviderUnavailable)
        ));
        let body =
            serde_json::to_vec(&json!({"id":c.profile.simple().to_string(),"name":"pLaYeR"}))
                .unwrap();
        assert_eq!(
            session_response(
                &c,
                "/session/minecraft/hasJoined",
                200,
                "application/json",
                &body
            )
            .unwrap(),
            "pLaYeR"
        );
        c.provider = "elyby".into();
        assert!(matches!(
            session_response(
                &c,
                "/session/hasJoined",
                401,
                "application/json",
                br#"{"error":"ForbiddenOperationException","errorMessage":"Invalid token."}"#
            ),
            Err(CloudError::IdentityProfileMismatch)
        ));
        c.provider = "drasl_unmojang".into();
        assert!(matches!(
            session_response(&c, "/session/minecraft/hasJoined", 403, "", b""),
            Err(CloudError::IdentityProfileMismatch)
        ));
    }
}
