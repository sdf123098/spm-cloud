use spm_cloud::{api, config::CloudConfig, realtime, store::CloudStore};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let config = CloudConfig::from_env()?;
    let store = CloudStore::open(&config)?;
    let state = api::AppState::new(config.clone(), store);
    let app = api::router(state.clone())
        .merge(realtime::router(state))
        .layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(address = %config.bind_addr, instance = %config.instance_id, "SPM Cloud listening");
    axum::serve(listener, app).await?;
    Ok(())
}
