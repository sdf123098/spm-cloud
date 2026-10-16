use spm_cloud::{
    api,
    config_file::{LoadedConfig, current_environment, init_config, select_file},
    store::CloudStore,
};

use std::path::PathBuf;
use tokio::net::TcpListener;
use tokio::time::{Duration, sleep};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = Options::parse(std::env::args_os().skip(1).collect())?;
    if options.help {
        println!(
            "spm-cloud [--config PATH] [--check-config | --print-effective-config | --enable-web-ui]\nspm-cloud --init-config PATH\nspm-cloud --version\nSelected JSON configuration is reloaded automatically. No selected file keeps the legacy environment mode."
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
    if options.enable_web_ui {
        spm_cloud::config_file::enable_web_ui(
            config_path
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--enable-web-ui requires --config PATH"))?,
        )?;
        println!(
            "WebUI enabled in the selected configuration. The running file watcher or next startup will apply it."
        );
        return Ok(());
    }
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
            .with_writer(spm_cloud::maintenance::LogWriter)
            .with_ansi(false)
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(spm_cloud::maintenance::LogWriter)
            .with_ansi(false)
            .with_env_filter(filter)
            .init();
    }
    let config = loaded.cloud.clone();
    let store = CloudStore::open(&config)?;
    let state = api::AppState::new(config.clone(), store.clone());
    state.initialize_settings(&loaded);
    {
        let _lock = state.config_edit_guard().await;
        spm_cloud::config_apply::apply(&state, &loaded, None, None)?;
    }
    spm_cloud::maintenance::start_update_loop(state.clone());
    if let Some(path) = config_path {
        tokio::spawn(reload_config(path, state.clone()));
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
    let compat_origins = std::env::var("SPM_CLOUD_COMPAT_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter(|origin| !origin.trim().is_empty())
        .map(|origin| origin.trim().to_owned())
        .collect::<Vec<_>>();
    let app = spm_cloud::compat::router(state, &compat_origins)?.layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(address = %config.bind_addr, instance = %config.instance_id, "SPM Cloud listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

async fn reload_config(path: PathBuf, state: api::AppState) {
    let mut previous = None::<(Vec<u8>, u64)>;
    loop {
        sleep(Duration::from_secs(2)).await;
        let _lock = state.config_edit_guard().await;
        let contents = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(_) => {
                tracing::warn!("unable to read Cloud JSON configuration; keeping active settings");
                continue;
            }
        };
        if previous.as_ref().is_some_and(|(bytes, generation)| {
            bytes == &contents && *generation == state.settings_generation()
        }) {
            continue;
        }
        let loaded = match LoadedConfig::load(Some(&path)) {
            Ok(loaded) => loaded,
            Err(_) => {
                tracing::warn!("invalid Cloud JSON update; keeping active settings");
                continue;
            }
        };
        if spm_cloud::config_apply::apply(&state, &loaded, None, None).is_err() {
            tracing::error!("failed to apply Cloud JSON update; keeping active settings");
            continue;
        }
        tracing::info!(
            "Cloud JSON runtime settings reloaded; startup-only changes remain pending until restart"
        );
        previous = Some((contents, state.settings_generation()));
    }
}
#[derive(Default)]
struct Options {
    config: Option<PathBuf>,
    init: Option<PathBuf>,
    check: bool,
    enable_web_ui: bool,
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
                Some("--enable-web-ui") if !result.enable_web_ui => result.enable_web_ui = true,
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
        anyhow::ensure!(
            !result.enable_web_ui
                || (result.config.is_some()
                    && !result.check
                    && !result.print
                    && result.init.is_none()
                    && !result.help
                    && !result.version),
            "--enable-web-ui requires --config PATH and cannot be combined with other operations"
        );
        anyhow::ensure!(
            !result.version
                || (result.config.is_none()
                    && result.init.is_none()
                    && !result.check
                    && !result.print
                    && !result.help
                    && !result.enable_web_ui),
            "--version cannot be combined with other operations"
        );
        Ok(result)
    }
}
