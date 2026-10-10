use spm_cloud::{
    api,
    config_file::{LoadedConfig, Logging, current_environment, init_config, select_file},
    realtime,
    store::CloudStore,
};
use std::path::Path;
use std::path::PathBuf;
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = Options::parse(std::env::args_os().skip(1).collect())?;
    if options.help {
        println!(
            "spm-cloud [--config PATH] [--check-config | --print-effective-config]\nspm-cloud --init-config PATH\nspm-cloud --version\nSelected JSON configuration is reloaded automatically. No selected file keeps the legacy environment mode."
        );
        return Ok(());
    }
    if options.version {
        println!("spm-cloud {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let Some(path) = &options.init {
        init_config(path)?;
        println!(
            "Configuration initialized at {}. Set the external HTTPS origin, restrict secret file permissions to the service account, then run --config PATH --check-config.",
            path.display()
        );
        return Ok(());
    }
    let cwd = std::env::current_dir()?;
    let environment = current_environment()?;
    let config_path = select_file(options.config.as_deref(), &cwd, &environment)?;
    let loaded = LoadedConfig::load_with(config_path.as_deref(), &cwd, &environment)?;
    if options.print {
        println!(
            "{}",
            serde_json::to_string_pretty(loaded.effective_report())?
        );
        return Ok(());
    }
    if options.check {
        println!(
            "Configuration valid. No database, account, listener or external authentication request was created."
        );
        return Ok(());
    }
    let filter = tracing_subscriber::EnvFilter::try_new(&loaded.log_filter)?;
    if loaded.logging.format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
    let config = loaded.cloud;
    let providers = loaded.providers;
    let logging = loaded.logging.clone();
    let log_filter = loaded.log_filter.clone();
    let store = CloudStore::open(&config)?;
    for provider in providers {
        store.configure_provider(&provider)?;
    }
    let state = api::AppState::new(config.clone(), store.clone());
    if let Some(path) = config_path {
        tokio::spawn(reload_config(
            path,
            state.clone(),
            store.clone(),
            logging,
            log_filter,
        ));
    }
    let cleanup_state = state.clone();
    let cleanup_store = store.clone();
    let cleanup_dir = config.object_dir.clone();
    tokio::spawn(async move {
        let mut last_visual_cleanup = 0;
        loop {
            let time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let visual_cleanup_interval = cleanup_state
                .runtime_config()
                .visual
                .retention
                .cleanup_interval_seconds;
            if time - last_visual_cleanup >= visual_cleanup_interval as i64 {
                if let Err(error) = cleanup_store.cleanup_visual_states(time * 1000) {
                    tracing::warn!(%error, "visual display cleanup failed");
                }
                last_visual_cleanup = time;
            }
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
            sleep(Duration::from_secs(1)).await;
        }
    });
    let app = api::router(state.clone())
        .merge(realtime::router(state))
        .layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(address = %config.bind_addr, instance = %config.instance_id, "SPM Cloud listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

async fn reload_config(
    path: PathBuf,
    state: api::AppState,
    store: CloudStore,
    mut current_logging: Logging,
    mut current_log_filter: String,
) {
    let mut previous = None::<Vec<u8>>;
    loop {
        sleep(Duration::from_secs(2)).await;
        let contents = match tokio::fs::read(&path).await {
            Ok(contents) => contents,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "unable to read Cloud JSON configuration; keeping active settings");
                continue;
            }
        };
        if previous.as_ref() == Some(&contents) {
            continue;
        }
        let loaded = match LoadedConfig::load(Some(Path::new(&path))) {
            Ok(loaded) => loaded,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "invalid Cloud JSON update; keeping active settings");
                continue;
            }
        };
        let current = state.runtime_config();
        let candidate = loaded.cloud;
        let restart_fields = [
            (
                "instance.instance_id",
                current.instance_id != candidate.instance_id,
            ),
            ("instance.origin", current.origin != candidate.origin),
            ("instance.bind", current.bind_addr != candidate.bind_addr),
            (
                "storage.database_path",
                current.database_path != candidate.database_path,
            ),
            (
                "storage.object_dir",
                current.object_dir != candidate.object_dir,
            ),
            (
                "auth.bootstrap_account_id",
                current.bootstrap_account_id != candidate.bootstrap_account_id,
            ),
            (
                "auth.bootstrap_password_hash_file",
                current.bootstrap_password_hash != candidate.bootstrap_password_hash,
            ),
            (
                "logging.level",
                current_logging.level != loaded.logging.level,
            ),
            (
                "logging.format",
                current_logging.format != loaded.logging.format,
            ),
            ("logging.filter", current_log_filter != loaded.log_filter),
        ]
        .into_iter()
        .filter_map(|(name, changed)| changed.then_some(name))
        .collect::<Vec<_>>();

        let mut provider_error = None;
        for provider in &loaded.providers {
            if let Err(error) = store.configure_provider(provider) {
                provider_error = Some(error);
                break;
            }
        }
        if let Some(error) = provider_error {
            tracing::error!(%error, "failed to apply identity provider configuration; keeping active runtime settings");
            continue;
        }

        let mut next = current;
        next.allow_self_registration = candidate.allow_self_registration;
        next.access_token = candidate.access_token;
        next.max_asset_bytes = candidate.max_asset_bytes;
        next.max_message_bytes = candidate.max_message_bytes;
        next.max_entity_query_count = candidate.max_entity_query_count;
        next.visual = candidate.visual;
        next.trusted_proxy_ips = candidate.trusted_proxy_ips;
        state.update_runtime_config(next.clone());
        current_logging = loaded.logging;
        current_log_filter = loaded.log_filter;
        if restart_fields.is_empty() {
            tracing::info!(
                max_asset_bytes = next.max_asset_bytes,
                "Cloud JSON runtime settings reloaded"
            );
        } else {
            tracing::warn!(
                fields = %restart_fields.join(", "),
                "Cloud JSON updated; these startup-only settings require a service restart"
            );
        }
        previous = Some(contents);
    }
}

#[derive(Default)]
struct Options {
    config: Option<PathBuf>,
    init: Option<PathBuf>,
    check: bool,
    print: bool,
    help: bool,
    version: bool,
}
impl Options {
    fn parse(args: Vec<std::ffi::OsString>) -> anyhow::Result<Self> {
        let mut result = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--config" | "--init-config") => {
                    let path = args
                        .next()
                        .filter(|p| !p.is_empty() && !p.to_string_lossy().starts_with("--"))
                        .ok_or_else(|| {
                            anyhow::anyhow!("{} requires a path", arg.to_string_lossy())
                        })?;
                    let destination = if arg == "--config" {
                        &mut result.config
                    } else {
                        &mut result.init
                    };
                    anyhow::ensure!(
                        destination.is_none(),
                        "duplicate option {}",
                        arg.to_string_lossy()
                    );
                    *destination = Some(path.into());
                }
                Some("--check-config") if !result.check => result.check = true,
                Some("--print-effective-config") if !result.print => result.print = true,
                Some("--help" | "-h") => result.help = true,
                Some("--version" | "-V") => result.version = true,
                _ => anyhow::bail!("unknown or duplicate option; use --help"),
            }
        }
        anyhow::ensure!(
            !(result.check && result.print),
            "choose --check-config or --print-effective-config"
        );
        anyhow::ensure!(
            result.init.is_none()
                || (result.config.is_none() && !result.check && !result.print && !result.help),
            "--init-config cannot be combined with other options"
        );
        anyhow::ensure!(
            !result.help || (result.config.is_none() && !result.check && !result.print),
            "--help cannot be combined with other options"
        );
        Ok(result)
    }
}
