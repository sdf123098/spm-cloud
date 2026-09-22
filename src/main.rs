use spm_cloud::{api, config::CloudConfig, realtime, store::CloudStore};
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let config = CloudConfig::from_env()?;
    let store = CloudStore::open(&config)?;
    let cleanup_store = store.clone();
    let cleanup_dir = config.object_dir.clone();
    tokio::spawn(async move {
        loop {
            if let Ok(paths) = cleanup_store.reconcile_upload_leases(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            ) {
                for path in paths {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
            if let Ok(mut entries) = tokio::fs::read_dir(&cleanup_dir).await {
                let stale_before = std::time::SystemTime::now()
                    .checked_sub(std::time::Duration::from_secs(15 * 60));
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if entry.file_name().to_string_lossy().starts_with(".upload-") {
                        let stale = stale_before
                            .zip(entry.metadata().await.ok())
                            .and_then(|(cutoff, metadata)| {
                                metadata.modified().ok().map(|modified| modified < cutoff)
                            })
                            .unwrap_or(false);
                        if stale {
                            let _ = tokio::fs::remove_file(entry.path()).await;
                        }
                    }
                }
            }
            sleep(Duration::from_secs(60)).await;
        }
    });
    let state = api::AppState::new(config.clone(), store);
    let app = api::router(state.clone())
        .merge(realtime::router(state))
        .layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(address = %config.bind_addr, instance = %config.instance_id, "SPM Cloud listening");
    axum::serve(listener, app).await?;
    Ok(())
}
