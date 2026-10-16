//! Browser game authentication. Provider tokens/passwords never reach persistent sessions.
use crate::database::Connection;
use crate::{CloudStore, account_links::profile_account, api::AppState, error::CloudError};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;
const CALLBACK: &str = "/admin/oauth/microsoft/callback";
const COOKIE: &str = "spm_microsoft_flow";
fn nonce() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}
fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
fn now() -> i64 {
    crate::api::now_unix_ms() / 1000
}

pub(crate) fn migrate(conn: &Connection) -> Result<(), CloudError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS browser_auth_flows(state_hash TEXT PRIMARY KEY, browser_hash TEXT NOT NULL, expires_at INTEGER NOT NULL, data TEXT NOT NULL);")?;
    Ok(())
}
#[derive(Serialize, Deserialize, Clone)]
struct VerifiedProfile {
    provider: String,
    uuid: Uuid,
    name: String,
}
#[derive(Serialize, Deserialize)]
struct Flow {
    purpose: String,
    account: Option<String>,
    session_hash: Option<String>,
    admin: bool,
    client_id: String,
    redirect_uri: String,
    verifier: String,
    profile: Option<VerifiedProfile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    purpose: String,
    #[serde(default)]
    admin: bool,
}

pub(crate) fn validate_settings(
    settings: &crate::admin::WebUiSettings,
    origin: &str,
) -> Result<(), CloudError> {
    if let Some(id) = &settings.microsoft_client_id {
        Uuid::parse_str(id).map_err(|_| {
            CloudError::invalid_metadata(
                "web_ui.microsoft_client_id: Microsoft Client ID 必须是有效 UUID",
            )
        })?;
    }
    let uri = settings
        .microsoft_redirect_uri
        .clone()
        .unwrap_or_else(|| format!("{origin}{CALLBACK}"));
    let url = reqwest::Url::parse(&uri).map_err(|_| {
        CloudError::invalid_metadata("web_ui.microsoft_redirect_uri: 微软回调地址格式不正确")
    })?;
    let local = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if !(url.scheme() == "https" || (local && url.scheme() == "http"))
        || url.path() != CALLBACK
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(CloudError::invalid_metadata(
            "web_ui.microsoft_redirect_uri: 微软回调地址必须使用 HTTPS（本机测试可使用 HTTP），并以 /admin/oauth/microsoft/callback 结尾",
        ));
    }
    Ok(())
}
fn actor_for(
    state: &AppState,
    headers: &HeaderMap,
    purpose: &str,
) -> Result<(Option<String>, Option<String>), CloudError> {
    match purpose {
        "login" => Ok((None, None)),
        "link" => {
            let (web, _) = profile_account(state, headers)?;
            let token = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .ok_or(CloudError::Unauthenticated)?;
            Ok((Some(web), Some(hash(token))))
        }
        _ => Err(CloudError::invalid_metadata(
            "purpose must be login or link",
        )),
    }
}
fn cookie(headers: &HeaderMap) -> Result<String, CloudError> {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.split(';').find_map(|p| {
                p.trim()
                    .strip_prefix(&format!("{COOKIE}="))
                    .map(str::to_owned)
            })
        })
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(CloudError::Unauthenticated)
}
fn store_flow(
    store: &CloudStore,
    state: &str,
    browser: &str,
    flow: &Flow,
    ttl: i64,
) -> Result<(), CloudError> {
    let conn = store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    conn.execute(
        "DELETE FROM browser_auth_flows WHERE expires_at<=?1",
        [now()],
    )?;
    conn.execute("INSERT INTO browser_auth_flows(state_hash,browser_hash,expires_at,data) VALUES(?1,?2,?3,?4)",params![hash(state),hash(browser),now()+ttl,serde_json::to_string(flow).unwrap()])?;
    Ok(())
}
fn take_flow(store: &CloudStore, state: &str, browser: &str) -> Result<Flow, CloudError> {
    if state.len() != 64 {
        return Err(CloudError::IdentityChallengeExpired);
    }
    let mut conn = store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let tx = conn.transaction()?;
    let data:Option<String>=tx.query_row("SELECT data FROM browser_auth_flows WHERE state_hash=?1 AND browser_hash=?2 AND expires_at>?3",params![hash(state),hash(browser),now()],|r|r.get(0)).optional()?;
    let data = data.ok_or(CloudError::IdentityChallengeExpired)?;
    tx.execute(
        "DELETE FROM browser_auth_flows WHERE state_hash=?1",
        [hash(state)],
    )?;
    tx.commit()?;
    serde_json::from_str(&data).map_err(|_| CloudError::configuration("invalid stored OAuth flow"))
}
fn validate_actor(state: &AppState, flow: &Flow) -> Result<(), CloudError> {
    if let Some(web) = &flow.account {
        state
            .store
            .admin_role(web, &state.config.bootstrap_account_id)?;
        let expected = flow
            .session_hash
            .as_deref()
            .ok_or(CloudError::Unauthenticated)?;
        let cfg = state.runtime_config();
        if web == &cfg.bootstrap_account_id
            && cfg
                .access_token
                .as_ref()
                .is_some_and(|t| hash(t) == expected)
        {
            return Ok(());
        }
        let conn = state
            .store
            .connection
            .lock()
            .map_err(|_| CloudError::configuration("database lock poisoned"))?;
        let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE account_id=?1 AND access_hash=?2 AND revoked=0 AND access_expires_at>?3)",params![web,expected,now()],|r|r.get(0))?;
        if !valid {
            return Err(CloudError::Unauthenticated);
        }
    }
    Ok(())
}
pub async fn providers(State(state): State<AppState>) -> Result<Json<Value>, CloudError> {
    let runtime = state.runtime_config();
    Ok(Json(
        json!({"providers":state.store.list_providers()?,"microsoft_configured":runtime.web_ui.microsoft_client_id.is_some(),"microsoft_redirect_uri":runtime.web_ui.microsoft_redirect_uri.unwrap_or_else(||format!("{}{CALLBACK}",runtime.origin))}),
    ))
}
pub async fn microsoft_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Start>,
) -> Result<Response, CloudError> {
    crate::admin::login_attempt(&state)?;
    let cfg = state.runtime_config();
    validate_settings(&cfg.web_ui, &cfg.origin)?;
    let id = cfg.web_ui.microsoft_client_id.clone().ok_or_else(|| {
        CloudError::invalid_metadata("服主尚未配置微软登录应用，请先使用 Cloud 账号登录")
    })?;
    let redirect = cfg
        .web_ui
        .microsoft_redirect_uri
        .clone()
        .unwrap_or_else(|| format!("{}{CALLBACK}", cfg.origin));
    let redirect_url = reqwest::Url::parse(&redirect).unwrap();
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if origin != redirect_url.origin().ascii_serialization() {
            return Err(CloudError::invalid_metadata(format!(
                "请通过 {} 打开控制台后使用微软登录",
                redirect_url.origin().ascii_serialization()
            )));
        }
    }
    let (account, session_hash) = actor_for(&state, &headers, &input.purpose)?;
    let state_id = nonce();
    let browser = nonce();
    let verifier = nonce();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let flow = Flow {
        purpose: input.purpose,
        account,
        session_hash,
        admin: input.admin,
        client_id: id.clone(),
        redirect_uri: redirect.clone(),
        verifier,
        profile: None,
    };
    store_flow(&state.store, &state_id, &browser, &flow, 600)?;
    let mut url =
        reqwest::Url::parse("https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize")
            .unwrap();
    url.query_pairs_mut()
        .append_pair("client_id", &id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_mode", "query")
        .append_pair("scope", "XboxLive.signin")
        .append_pair("state", &state_id)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("prompt", "select_account");
    let mut response = Json(json!({"url":url.as_str()})).into_response();
    response.headers_mut().insert("set-cookie",format!("{COOKIE}={browser}; HttpOnly; SameSite=Lax; Path=/admin/oauth/microsoft; Max-Age=600{}",if redirect_url.scheme()=="https"{"; Secure"}else{""}).parse().unwrap());
    Ok(response)
}
#[derive(Deserialize)]
pub struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}
pub async fn microsoft_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<Callback>,
) -> Result<Response, CloudError> {
    let browser = match cookie(&headers) {
        Ok(browser) => browser,
        Err(_) => {
            return Ok(callback_failure(
                "/admin/",
                CloudError::IdentityChallengeExpired,
            ));
        }
    };
    let mut flow = match take_flow(&state.store, input.state.as_deref().unwrap_or(""), &browser) {
        Ok(flow) => flow,
        Err(error) => return Ok(callback_failure("/admin/", error)),
    };
    let destination = if flow.admin { "/admin/" } else { "/account/" };
    let result = async {
        validate_actor(&state, &flow)?;
        if input.error.is_some() {
            return Err(CloudError::invalid_metadata("微软登录已取消或未获授权"));
        }
        let code = input
            .code
            .filter(|s| !s.is_empty() && s.len() <= 8192)
            .ok_or(CloudError::IdentityChallengeExpired)?;
        let client = client()?;
        let mut form = vec![
            ("client_id", flow.client_id.clone()),
            ("grant_type", "authorization_code".into()),
            ("code", code),
            ("redirect_uri", flow.redirect_uri.clone()),
            ("code_verifier", flow.verifier.clone()),
        ];
        if let Some(secret) = microsoft_client_secret(&state.runtime_config())? {
            form.push(("client_secret", secret));
        }
        let token = stage_json(
            client
                .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
                .form(&form),
            "微软应用授权",
        )
        .await?;
        let msa = required(&token, "access_token")?;
        let profile = microsoft_profile(&client, msa).await?;
        flow.verifier.clear();
        flow.profile = Some(profile);
        let ticket = nonce();
        store_flow(&state.store, &ticket, &browser, &flow, 90)?;
        Ok::<_, CloudError>(ticket)
    }
    .await;
    Ok(match result {
        Ok(ticket) => Redirect::to(&format!("{destination}#oauth={ticket}")).into_response(),
        Err(error) => callback_failure(destination, error),
    })
}
fn callback_failure(destination: &str, error: CloudError) -> Response {
    let message = match &error {
        CloudError::IdentityChallengeExpired => {
            "微软登录验证已过期或浏览器 Cookie 已丢失，请在原浏览器重新登录 Cloud 并开始验证。"
        }
        CloudError::Unauthenticated | CloudError::SessionExpired => {
            "原 Cloud 登录会话已失效，请重新登录 Cloud 后再开始微软验证。"
        }
        CloudError::InvalidMetadata(message) => message.as_str(),
        _ => "微软登录验证未完成，请重新开始；若仍失败，请检查身份服务与应用配置。",
    };
    let mut url = reqwest::Url::parse("https://localhost/").unwrap();
    url.query_pairs_mut()
        .append_pair("oauth_error", error.code())
        .append_pair("oauth_message", message);
    Redirect::to(&format!("{destination}#{}", url.query().unwrap())).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redeem {
    ticket: String,
}
pub async fn microsoft_redeem(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Redeem>,
) -> Result<Json<Value>, CloudError> {
    let flow = take_flow(&state.store, &input.ticket, &cookie(&headers)?)?;
    validate_actor(&state, &flow)?;
    let profile = flow.profile.ok_or(CloudError::IdentityChallengeExpired)?;
    Ok(Json(finish(
        &state,
        &flow.purpose,
        flow.account.as_deref(),
        flow.admin,
        &profile,
    )?))
}
fn client() -> Result<reqwest::Client, CloudError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|_| CloudError::IdentityProviderUnavailable)
}
async fn read_json(request: reqwest::RequestBuilder) -> Result<Value, CloudError> {
    stage_json(request, "游戏身份验证").await
}
async fn stage_json(request: reqwest::RequestBuilder, stage: &str) -> Result<Value, CloudError> {
    use futures_util::StreamExt;
    let response = request
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|_| CloudError::IdentityProviderUnavailable)?;
    if !response.status().is_success() {
        // Provider bodies can contain tokens: report only the stage and HTTP status.
        return Err(CloudError::invalid_metadata(format!(
            "{stage}失败（HTTP {}），请检查应用权限、账号和服务配置",
            response.status().as_u16()
        )));
    }
    let mut data = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| CloudError::IdentityProviderUnavailable)?;
        if data.len() + chunk.len() > 524288 {
            return Err(CloudError::IdentityProviderUnavailable);
        }
        data.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&data).map_err(|_| CloudError::IdentityProviderUnavailable)
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, CloudError> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(CloudError::IdentityProfileMismatch)
}
fn parsed_profile(provider: &str, value: &Value) -> Result<VerifiedProfile, CloudError> {
    let uuid =
        Uuid::parse_str(required(value, "id")?).map_err(|_| CloudError::IdentityProfileMismatch)?;
    let name = required(value, "name")?;
    if name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(CloudError::IdentityProfileMismatch);
    }
    Ok(VerifiedProfile {
        provider: provider.into(),
        uuid,
        name: name.into(),
    })
}
async fn minecraft_profile(
    client: &reqwest::Client,
    token: &str,
) -> Result<VerifiedProfile, CloudError> {
    let value = stage_json(
        client
            .get("https://api.minecraftservices.com/minecraft/profile")
            .bearer_auth(token),
        "Minecraft 正版角色验证",
    )
    .await?;
    parsed_profile("official", &value)
}
async fn microsoft_profile(
    client: &reqwest::Client,
    msa: &str,
) -> Result<VerifiedProfile, CloudError> {
    let user=stage_json(client.post("https://user.auth.xboxlive.com/user/authenticate").json(&json!({"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={msa}")},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"})),"Xbox 账号验证").await?;
    let xsts=stage_json(client.post("https://xsts.auth.xboxlive.com/xsts/authorize").json(&json!({"Properties":{"SandboxId":"RETAIL","UserTokens":[required(&user,"Token")?]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"})),"Xbox 游戏授权").await?;
    let uhs = xsts["DisplayClaims"]["xui"][0]["uhs"]
        .as_str()
        .ok_or(CloudError::IdentityProfileMismatch)?;
    let mc = stage_json(
        client
            .post("https://api.minecraftservices.com/authentication/login_with_xbox")
            .json(&json!({"identityToken":format!("XBL3.0 x={uhs};{}",required(&xsts,"Token")?)})),
        "Minecraft 应用授权",
    )
    .await?;
    minecraft_profile(client, required(&mc, "access_token")?).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameInput {
    purpose: String,
    #[serde(default)]
    admin: bool,
    provider_id: String,
    username: Option<String>,
    password: Option<String>,
    access_token: Option<String>,
    profile_uuid: Option<Uuid>,
    profile_name: Option<String>,
}
pub async fn game(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<GameInput>,
) -> Result<Json<Value>, CloudError> {
    crate::admin::login_attempt(&state)?;
    let (account, _) = actor_for(&state, &headers, &input.purpose)?;
    let profile = if input.provider_id == "official" {
        let token = input
            .access_token
            .as_deref()
            .filter(|s| !s.is_empty() && s.len() <= 16384)
            .ok_or_else(|| {
                CloudError::invalid_metadata(
                    "请使用微软网页登录，或在手动验证中填写 Minecraft 访问令牌",
                )
            })?;
        minecraft_profile(&client()?, token).await?
    } else {
        let (base, path, custom_auth_path) = provider(&state.store, &input.provider_id)?;
        let (client, base) = provider_client(&base).await?;
        let auth_path = custom_auth_path
            .as_deref()
            .unwrap_or(if path == "/session/hasJoined" {
                "/auth/authenticate"
            } else {
                "/authserver/authenticate"
            });
        let token;
        let selected;
        if let Some(access) = &input.access_token {
            if access.is_empty() || access.len() > 16384 {
                return Err(CloudError::IdentityProfileMismatch);
            }
            token = access.clone();
            selected = json!({"id":input.profile_uuid.ok_or_else(||CloudError::invalid_metadata("手动第三方验证需要角色 UUID"))?,"name":input.profile_name.as_deref().ok_or_else(||CloudError::invalid_metadata("手动第三方验证需要角色名称"))?});
        } else {
            let username = input
                .username
                .as_deref()
                .filter(|s| !s.is_empty() && s.len() <= 256)
                .ok_or(CloudError::Unauthenticated)?;
            let password = input
                .password
                .as_deref()
                .filter(|s| !s.is_empty() && s.len() <= 4096)
                .ok_or(CloudError::Unauthenticated)?;
            let value=read_json(client.post(format!("{base}{auth_path}")).json(&json!({"agent":{"name":"Minecraft","version":1},"username":username,"password":password,"requestUser":false}))).await?;
            token = required(&value, "accessToken")?.to_owned();
            let profiles = value["availableProfiles"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            selected = if let Some(uuid) = input.profile_uuid {
                profiles
                    .iter()
                    .find(|p| p["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()) == Some(uuid))
                    .cloned()
                    .or_else(|| {
                        (value["selectedProfile"]["id"]
                            .as_str()
                            .and_then(|s| Uuid::parse_str(s).ok())
                            == Some(uuid))
                        .then(|| value["selectedProfile"].clone())
                    })
                    .ok_or(CloudError::IdentityProfileMismatch)?
            } else if value["selectedProfile"].is_object() {
                value["selectedProfile"].clone()
            } else if profiles.len() == 1 {
                profiles[0].clone()
            } else {
                return Ok(Json(json!({"choose_profile":true,"profiles":profiles})));
            };
        }
        let profile = parsed_profile(&input.provider_id, &selected)?;
        let server_id = nonce();
        let join_path = path
            .strip_suffix("hasJoined")
            .map(|s| format!("{s}join"))
            .ok_or(CloudError::IdentityProviderUntrusted)?;
        let join=client.post(format!("{base}{join_path}")).json(&json!({"accessToken":token,"selectedProfile":profile.uuid.simple().to_string(),"serverId":server_id})).send().await.map_err(|_|CloudError::IdentityProviderUnavailable)?;
        if !join.status().is_success() {
            return Err(CloudError::IdentityProfileMismatch);
        }
        let joined = read_json(client.get(format!("{base}{path}")).query(&[
            ("username", profile.name.as_str()),
            ("serverId", server_id.as_str()),
        ]))
        .await?;
        let verified = parsed_profile(&input.provider_id, &joined)?;
        if verified.uuid != profile.uuid || !verified.name.eq_ignore_ascii_case(&profile.name) {
            return Err(CloudError::IdentityProfileMismatch);
        }
        verified
    };
    Ok(Json(finish(
        &state,
        &input.purpose,
        account.as_deref(),
        input.admin,
        &profile,
    )?))
}
fn provider(store: &CloudStore, id: &str) -> Result<(String, String, Option<String>), CloudError> {
    let conn = store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    conn.query_row("SELECT base_url,session_path,authenticate_path FROM identity_providers WHERE provider_id=?1 AND enabled=1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(CloudError::IdentityProviderUntrusted)
}
fn microsoft_client_secret(config: &crate::CloudConfig) -> Result<Option<String>, CloudError> {
    let secret = if let Some(secret) = &config.microsoft_client_secret {
        secret.clone()
    } else if let Some(path) = &config.web_ui.microsoft_client_secret_file {
        std::fs::read_to_string(path)
            .map_err(|_| CloudError::configuration("cannot read Microsoft secret file"))?
    } else {
        return Ok(None);
    };
    if secret.trim().is_empty() || secret.len() > 4096 {
        return Err(CloudError::configuration("invalid Microsoft secret file"));
    }
    Ok(Some(secret.trim_start_matches('\u{feff}').trim().into()))
}
async fn provider_client(base: &str) -> Result<(reqwest::Client, String), CloudError> {
    let (client, url) = crate::game_auth::provider_client(base, Duration::from_secs(25)).await?;
    Ok((client, url.as_str().trim_end_matches('/').to_owned()))
}
fn finish(
    state: &AppState,
    purpose: &str,
    web: Option<&str>,
    admin: bool,
    profile: &VerifiedProfile,
) -> Result<Value, CloudError> {
    if purpose == "link" {
        // The provider may have been disabled while the upstream request was in flight.
        provider(&state.store, &profile.provider)?;
        let web = web.ok_or(CloudError::Unauthenticated)?;
        state
            .store
            .admin_role(web, &state.config.bootstrap_account_id)?;
        let mut cloud = state.store.linked_cloud_account(web)?;
        let existing = identity_owner(&state.store, profile)?;
        if let Some(existing) = existing {
            if existing != cloud {
                if cloud != web {
                    return Err(CloudError::IdentityAlreadyLinked);
                }
                // The verified game identity proves access to the already-linked Cloud account.
                state.store.connect_owned_accounts(web, &existing)?;
                cloud = existing;
            }
        }
        state
            .store
            .admin_role(&cloud, &state.config.bootstrap_account_id)?;
        let identity = state.store.finish_game_identity(
            "link",
            Some(&cloud),
            &profile.provider,
            profile.uuid,
            &profile.name,
            Some(&profile.name),
        )?;
        state.store.admin_audit(
            web,
            "SELF_GAME_IDENTITY_LINKED",
            Some(&cloud),
            json!({"provider_id":profile.provider,"profile_uuid":profile.uuid}),
        )?;
        let mut response = issue(state, web, false)?;
        response["linked_identity"] = identity;
        response["cloud_account_id"] = json!(cloud);
        return Ok(response);
    }
    if purpose != "login" {
        return Err(CloudError::invalid_metadata("invalid login purpose"));
    }
    let cloud = identity_owner(&state.store, profile)?.ok_or(CloudError::IdentityNotLinked)?;
    provider(&state.store, &profile.provider)?;
    state
        .store
        .admin_role(&cloud, &state.config.bootstrap_account_id)?;
    let web = state.store.linked_web_account(&cloud)?;
    issue(state, &web, admin)
}
fn identity_owner(
    store: &CloudStore,
    profile: &VerifiedProfile,
) -> Result<Option<String>, CloudError> {
    let conn = store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let kind = if profile.provider == "official" {
        "official"
    } else {
        "yggdrasil"
    };
    let provider = if kind == "official" {
        ""
    } else {
        profile.provider.as_str()
    };
    let mut query=conn.prepare("SELECT DISTINCT account_id FROM identities WHERE identity_kind=?1 AND COALESCE(provider_id,'')=?2 AND profile_uuid=?3 AND verified=1")?;
    let accounts = query
        .query_map(params![kind, provider, profile.uuid.to_string()], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    match accounts.as_slice() {
        [] => Ok(None),
        [account] => Ok(Some(account.clone())),
        _ => Err(CloudError::IdentityAlreadyLinked),
    }
}
fn issue(state: &AppState, account: &str, admin: bool) -> Result<Value, CloudError> {
    let role = state
        .store
        .admin_role(account, &state.config.bootstrap_account_id)?;
    if admin && !matches!(role.as_str(), "owner" | "admin") {
        return Err(CloudError::AccessDenied);
    }
    let mut conn = state
        .store
        .connection
        .lock()
        .map_err(|_| CloudError::configuration("database lock poisoned"))?;
    let session = crate::store::issue_session_in_transaction(&mut conn, account)?;
    let mut value = serde_json::to_value(session).unwrap();
    value["account_id"] = json!(account);
    value["role"] = json!(role);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::tests::test_state;
    #[test]
    fn microsoft_environment_secret_reaches_token_exchange_without_disclosure() {
        use crate::config_file::{Environment, LoadedConfig};
        let dir = tempfile::tempdir().unwrap();
        let environment = Environment::from([(
            "SPM_CLOUD_MICROSOFT_CLIENT_SECRET".into(),
            "env-microsoft-secret".into(),
        )]);
        let loaded = LoadedConfig::load_with(None, dir.path(), &environment).unwrap();
        assert_eq!(
            microsoft_client_secret(&loaded.cloud).unwrap().as_deref(),
            Some("env-microsoft-secret")
        );
        assert!(
            !loaded
                .effective_report()
                .to_string()
                .contains("env-microsoft-secret")
        );
        assert!(!format!("{:?}", loaded.cloud).contains("env-microsoft-secret"));
        let path = dir.path().join("config.json");
        std::fs::write(
            dir.path().join("microsoft.txt"),
            "\u{feff}file-microsoft-secret\r\n",
        )
        .unwrap();
        std::fs::write(
            &path,
            r#"{"web_ui":{"microsoft_client_secret_file":"microsoft.txt"}}"#,
        )
        .unwrap();
        let configured = LoadedConfig::load_with(Some(&path), dir.path(), &environment).unwrap();
        assert_eq!(
            microsoft_client_secret(&configured.cloud)
                .unwrap()
                .as_deref(),
            Some("file-microsoft-secret")
        );
        let report = configured.effective_report().to_string();
        assert!(!report.contains("file-microsoft-secret"));
        assert!(!report.contains("env-microsoft-secret"));
    }
    fn flow() -> Flow {
        Flow {
            purpose: "login".into(),
            account: None,
            session_hash: None,
            admin: false,
            client_id: Uuid::new_v4().to_string(),
            redirect_uri: "http://localhost:18822/admin/oauth/microsoft/callback".into(),
            verifier: nonce(),
            profile: None,
        }
    }
    #[test]
    fn oauth_tickets_require_same_browser_expire_and_cannot_replay() {
        let (state, _dir) = test_state(false);
        let ticket = nonce();
        let browser = nonce();
        store_flow(&state.store, &ticket, &browser, &flow(), 60).unwrap();
        assert!(take_flow(&state.store, &ticket, &nonce()).is_err());
        assert!(take_flow(&state.store, &ticket, &browser).is_ok());
        assert!(take_flow(&state.store, &ticket, &browser).is_err());
        store_flow(&state.store, &ticket, &browser, &flow(), -1).unwrap();
        assert!(take_flow(&state.store, &ticket, &browser).is_err());
    }

    #[tokio::test]
    async fn expired_oauth_flow_returns_actionable_login_page() {
        let (state, _dir) = test_state(false);
        let ticket = nonce();
        let browser = nonce();
        store_flow(&state.store, &ticket, &browser, &flow(), -1).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("cookie", format!("{COOKIE}={browser}").parse().unwrap());
        let response = microsoft_callback(
            State(state),
            headers,
            Query(Callback {
                state: Some(ticket),
                code: Some("not-a-real-code".into()),
                error: None,
            }),
        )
        .await;
        assert!(
            response.is_ok(),
            "An expired state must return the login page rather than a JSON extractor error"
        );
        let response = response.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::SEE_OTHER);
        let location = response.headers()["location"].to_str().unwrap();
        assert!(location.starts_with("/admin/#oauth_error=IDENTITY_CHALLENGE_EXPIRED"));
        assert!(!location.contains("#oauth="));
    }
    #[test]
    fn oauth_binding_rechecks_original_session_and_callback_configuration() {
        let (state, _dir) = test_state(false);
        state
            .store
            .create_account("member", "a-test-password")
            .unwrap();
        let mut f = flow();
        f.account = Some("member".into());
        let login = issue(&state, "member", false).unwrap();
        let token = login["access_token"].as_str().unwrap();
        f.session_hash = Some(hash(token));
        validate_actor(&state, &f).unwrap();
        state.store.revoke_access_token(token).unwrap();
        assert!(validate_actor(&state, &f).is_err());
        let mut settings = crate::admin::WebUiSettings {
            microsoft_client_id: Some(f.client_id),
            microsoft_redirect_uri: Some(f.redirect_uri),
            ..Default::default()
        };
        validate_settings(&settings, "https://cloud.example").unwrap();
        for invalid in [
            "http://cloud.example/admin/oauth/microsoft/callback",
            "https://cloud.example/evil",
            "https://user:secret@cloud.example/admin/oauth/microsoft/callback",
            "https://cloud.example/admin/oauth/microsoft/callback?extra=1",
        ] {
            settings.microsoft_redirect_uri = Some(invalid.into());
            assert!(validate_settings(&settings, "https://cloud.example").is_err());
        }
    }
    #[test]
    fn full_binding_moves_game_identities_preserves_ids_and_keeps_web_roles() {
        let (state, _dir) = test_state(false);
        let store = &state.store;
        for name in ["web_user", "cloud_user", "another"] {
            store.create_account(name, "a-test-password").unwrap();
        }
        let p = VerifiedProfile {
            provider: "official".into(),
            uuid: Uuid::new_v4(),
            name: "Player".into(),
        };
        let old = store
            .finish_game_identity(
                "link",
                Some("web_user"),
                "official",
                p.uuid,
                &p.name,
                Some(&p.name),
            )
            .unwrap();
        store
            .connect_owned_accounts("web_user", "cloud_user")
            .unwrap();
        assert!(store.list_identities("web_user").unwrap().is_empty());
        let identities = store.list_identities("cloud_user").unwrap();
        assert_eq!(identities[0].identity_id, old["identity_id"]);
        assert_eq!(
            store
                .finish_game_identity("login", None, "official", p.uuid, &p.name, Some(&p.name))
                .unwrap()["account_id"],
            "cloud_user"
        );
        let web = finish(&state, "login", None, false, &p).unwrap();
        assert_eq!(web["account_id"], "web_user");
        assert_eq!(web["role"], "member");
        assert!(matches!(
            finish(&state, "login", None, true, &p),
            Err(CloudError::AccessDenied)
        ));
        assert!(matches!(
            store.connect_owned_accounts("another", "cloud_user"),
            Err(CloudError::IdentityAlreadyLinked)
        ));
        assert!(matches!(
            store.connect_owned_accounts("cloud_user", "another"),
            Err(CloudError::IdentityAlreadyLinked)
        ));
        let other_namespace = VerifiedProfile {
            provider: "ely".into(),
            ..p
        };
        assert!(matches!(
            finish(&state, "login", None, false, &other_namespace),
            Err(CloudError::IdentityNotLinked)
        ));
    }
    #[test]
    fn proving_existing_game_identity_links_accounts_without_stealing_identity() {
        let (state, _dir) = test_state(false);
        for name in ["web_user", "cloud_user", "other_web"] {
            state.store.create_account(name, "a-test-password").unwrap();
        }
        let p = VerifiedProfile {
            provider: "official".into(),
            uuid: Uuid::new_v4(),
            name: "Player".into(),
        };
        state
            .store
            .finish_game_identity(
                "link",
                Some("cloud_user"),
                "official",
                p.uuid,
                &p.name,
                Some(&p.name),
            )
            .unwrap();
        let result = finish(&state, "link", Some("web_user"), false, &p).unwrap();
        assert_eq!(result["cloud_account_id"], "cloud_user");
        assert_eq!(
            identity_owner(&state.store, &p).unwrap().as_deref(),
            Some("cloud_user")
        );
        assert!(matches!(
            finish(&state, "link", Some("other_web"), false, &p),
            Err(CloudError::IdentityAlreadyLinked)
        ));
    }
}
