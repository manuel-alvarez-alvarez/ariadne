//! ariadned — the Ariadne daemon.
//!
//! Serves the REST API on a unix socket (docker-style) and optionally on a
//! TCP listener for web/desktop frontends.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Parser;
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use ariadne_daemon::config::Config;
use ariadne_daemon::http::{self, AppState};

#[derive(Parser)]
#[command(
    name = "ariadned",
    version,
    about = "Ariadne coding-agent orchestrator daemon"
)]
struct Args {
    /// Ariadne home directory (default: $ARIADNE_HOME or ~/.ariadne)
    #[arg(long)]
    home: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(args.home)?;

    // RUST_LOG wins over config so ad-hoc debugging stays easy.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.log_filter.clone()));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!(root = %config.root.display(), "starting ariadned {}", env!("CARGO_PKG_VERSION"));

    let store = ariadne_store::Store::open(&config.db_path)
        .await
        .with_context(|| format!("opening database {}", config.db_path.display()))?;

    let plugin = ariadne_daemon::opencode_plugin::install()?;
    info!(plugin = %plugin.display(), "opencode events plugin installed");

    let unix_listener = bind_unix_socket(&config).await?;
    std::fs::write(&config.pid_file, std::process::id().to_string())
        .with_context(|| format!("writing {}", config.pid_file.display()))?;

    let config = std::sync::Arc::new(config);
    let launcher = std::sync::Arc::new(ariadne_daemon::launcher::Launcher {
        cfg: config.clone(),
        store: store.clone(),
        tmux: ariadne_daemon::tmux::TmuxManager::default(),
        git: ariadne_daemon::gitwt::GitManager,
    });

    let sched_tx = ariadne_daemon::scheduler::start(store.clone(), launcher.clone());
    let state = AppState {
        store,
        started_at: Instant::now(),
        launcher,
        sched_tx: Some(sched_tx),
    };
    let app = http::router(state);

    let shutdown = shutdown_signal();
    let result = match config.tcp_listen {
        Some(addr) => {
            let tcp_listener = TcpListener::bind(addr)
                .await
                .with_context(|| format!("binding tcp {addr}"))?;
            info!(%addr, "tcp listener enabled");
            let unix_srv =
                axum::serve(unix_listener, app.clone()).with_graceful_shutdown(shutdown_signal());
            let tcp_srv = axum::serve(tcp_listener, app).with_graceful_shutdown(shutdown);
            tokio::try_join!(unix_srv.into_future(), tcp_srv.into_future()).map(|_| ())
        }
        None => {
            axum::serve(unix_listener, app)
                .with_graceful_shutdown(shutdown)
                .await
        }
    };

    // Best-effort cleanup of runtime files.
    let _ = std::fs::remove_file(&config.socket_path);
    let _ = std::fs::remove_file(&config.pid_file);
    info!("ariadned stopped");

    result.context("http server error")
}

/// Bind the unix socket, refusing to start if another daemon already answers
/// on it, and cleaning up a stale socket file left by a crash.
async fn bind_unix_socket(config: &Config) -> Result<UnixListener> {
    let path = &config.socket_path;
    if path.exists() {
        let alive = tokio::time::timeout(Duration::from_secs(1), UnixStream::connect(path))
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false);
        if alive {
            bail!(
                "another ariadned is already listening on {} — stop it first",
                path.display()
            );
        }
        warn!(socket = %path.display(), "removing stale socket file");
        std::fs::remove_file(path)
            .with_context(|| format!("removing stale socket {}", path.display()))?;
    }

    let listener = UnixListener::bind(path)
        .with_context(|| format!("binding unix socket {}", path.display()))?;
    // Owner-only: the socket is the local trust boundary.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 600 {}", path.display()))?;
    info!(socket = %path.display(), "listening");
    Ok(listener)
}

/// Resolve on SIGINT (ctrl-c) or SIGTERM.
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
    }
    info!("shutdown signal received");
}
