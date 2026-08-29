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
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use ariadne_daemon::config::Config;
use ariadne_daemon::http::{self, AppState};

/// Everything that configures the daemon outside its own two flags, one line
/// each: `--help` is where an operator looks for it, and a key that is not
/// written down here is one nobody knows to set.
const ENVIRONMENT: &str = "\
Environment:
  ARIADNE_HOME  home directory: socket, database, worktrees, run dir and log
                (default: ~/.ariadne; --home wins over it)
  RUST_LOG      tracing filter for this run; wins over the log_filter below
                (e.g. info,ariadne_daemon=debug)

Configuration — <home>/config.toml, every key optional, read strictly (an
unknown key stops the daemon rather than being ignored):
  socket_path              unix socket to listen on (default: <home>/ariadne.sock)
  db_path                  SQLite database (default: <home>/ariadne.db)
  worktree_root            where task worktrees are created (default: <home>/worktrees)
  run_dir                  per-session run files: spawn plans, console logs
                           (default: <home>/run)
  tcp_listen               extra TCP listener for web/desktop UIs, e.g.
                           \"127.0.0.1:7676\" (default: unix socket only)
  log_filter               tracing filter when RUST_LOG says nothing (default: info)
  cli_bin                  the `ariadne` every session, hook and MCP server is
                           launched with (default: the one beside this binary)
  delete_merged_branches   delete a task branch once it has landed (default: true)
  delete_merged_worktrees  delete a task worktree once it has landed (default: true)
  prevent_sleep            hold off system sleep while a session is live (default: true)

  ariadned --check-config reads that file and exits.\
";

#[derive(Parser)]
#[command(
    name = "ariadned",
    version,
    about = "Ariadne coding-agent orchestrator daemon",
    after_help = ENVIRONMENT
)]
struct Args {
    /// Ariadne home directory (default: $ARIADNE_HOME or ~/.ariadne)
    #[arg(long)]
    home: Option<PathBuf>,
    /// Read <home>/config.toml, say what it resolves to, and exit
    ///
    /// Nothing is started, opened or created: it is the config the next start
    /// would run on, checked while the daemon that is running keeps running.
    #[arg(long)]
    check_config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.check_config {
        println!("{}", Config::check(args.home)?);
        return Ok(());
    }
    let config = Config::load(args.home)?;

    // RUST_LOG wins over config so ad-hoc debugging stays easy.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.log_filter.clone()));
    // Everything that passes the filter goes to stdout as before, and into
    // the in-memory buffer behind `/v1/logs`.
    let logs = ariadne_daemon::log::LogBuffer::new();
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(logs.layer())
        .init();

    info!(root = %config.root.display(), "starting ariadned {}", env!("CARGO_PKG_VERSION"));

    let store = ariadne_store::Store::open(&config.db_path)
        .await
        .with_context(|| format!("opening database {}", config.db_path.display()))?;

    // Installed before anything writes, so no state change goes unannounced.
    let events = ariadne_daemon::bus::start(store.clone());

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
        branches: ariadne_daemon::branch::BranchWatchers::new(events.clone()),
    });
    // The watches are the process's own: whatever was in flight when the last
    // daemon stopped is picked up again here.
    if let Err(e) = launcher.watch_task_branches().await {
        warn!(error = %e, "cannot follow the branches of the tasks already in flight");
    }

    let sched_tx =
        ariadne_daemon::scheduler::start(store.clone(), launcher.clone(), config.prevent_sleep);
    let state = AppState {
        store,
        started_at: Instant::now(),
        started_at_utc: chrono::Utc::now(),
        launcher,
        sched_tx: Some(sched_tx),
        events,
        logs,
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
