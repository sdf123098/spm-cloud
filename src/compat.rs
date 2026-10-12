//! Explicit origin aliases share the same database, runtime settings and realtime bus.
use crate::{
    api::{self, AppState},
    config::normalize_origin,
    error::CloudError,
    realtime,
};
use axum::{
    Router,
    extract::{ConnectInfo, Request},
    routing::any,
};
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

pub fn router(state: AppState, origins: &[String]) -> Result<Router, CloudError> {
    let canonical = api::router(state.clone()).merge(realtime::router(state.clone()));
    let mut aliases = Vec::new();
    for origin in origins {
        let origin = normalize_origin(origin)?;
        let host = reqwest::Url::parse(&origin).unwrap().authority().to_owned();
        if origin == state.config.origin {
            continue;
        }
        let mut alias = state.clone();
        let mut config = (*alias.config).clone();
        config.origin = origin.clone();
        alias.config = Arc::new(config);
        aliases.push((
            host,
            origin,
            api::router(alias.clone()).merge(realtime::router(alias)),
        ));
    }
    Ok(Router::new().fallback(any(move |request: Request| {
        let host = request
            .headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let trusted = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|peer| {
                state
                    .runtime_config()
                    .trusted_proxy_ips
                    .contains(&peer.0.ip())
            });
        let forwarded = trusted
            .then(|| {
                request
                    .headers()
                    .get("x-spm-compat-origin")
                    .and_then(|v| v.to_str().ok())
            })
            .flatten();
        let selected = aliases
            .iter()
            .find(|(alias_host, origin, _)| {
                host.eq_ignore_ascii_case(alias_host) || forwarded == Some(origin.as_str())
            })
            .map(|(_, _, router)| router)
            .unwrap_or(&canonical)
            .clone();
        async move { selected.oneshot(request).await }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request as HttpRequest,
    };
    #[tokio::test]
    async fn aliases_advertise_the_selected_origin_and_ignore_untrusted_forwarding() {
        let (state, _dir) = crate::api::tests::test_state(false);
        let canonical = state.config.origin.clone();
        let mut runtime = state.runtime_config();
        runtime.trusted_proxy_ips = vec!["127.0.0.1".parse().unwrap()];
        state.update_runtime_config(runtime).unwrap();
        let app = router(state, &["https://old.example.com".into()]).unwrap();
        for (host, forwarded, expected) in [
            ("old.example.com", "", "https://old.example.com"),
            ("localhost", "https://old.example.com", canonical.as_str()),
            ("unknown.example.com", "", canonical.as_str()),
        ] {
            let response = app
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .uri("/v1/instance")
                        .header("host", host)
                        .header("x-spm-compat-origin", forwarded)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let value: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                    .unwrap();
            assert_eq!(value["origin"], expected);
        }
        let mut request = HttpRequest::builder()
            .uri("/v1/instance")
            .header("host", "localhost")
            .header("x-spm-compat-origin", "https://old.example.com")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
        let response = app.oneshot(request).await.unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(value["origin"], "https://old.example.com");
    }
}
