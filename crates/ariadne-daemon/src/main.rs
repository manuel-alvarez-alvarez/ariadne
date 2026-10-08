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
  run_dir                  per-session run files: ACP launch files, skills
                           (default: <home>/run)
  tcp_listen               extra TCP listener for web/desktop UIs, e.g.
                           \"127.0.0.1:7676\" (default: unix socket only)
  webhook_listen           signed webhook listener (default: 127.0.0.1:0)
  webhook_public_url       public URL forwarded to the webhook listener; set, it
                           opens no tunnel
  tunnel_host              the localtunnel server the webhook tunnel registers
                           with (default: https://localtunnel.me)
  tunnel_subdomain         the subdomain the tunnel asks for (default: the
                           stored one, else a random one kept on first use)
  log_filter               tracing filter when RUST_LOG says nothing (default: info)
  cli_bin                  the `ariadne` every session's MCP server is launched
                           with (default: the one beside this binary)
  delete_merged_branches   delete a task branch once it has landed (default: true)
  delete_merged_worktrees  delete a task worktree once it has landed (default: true)
  prevent_sleep            hold off system sleep while a session is live (default: true)
  auto_switch              switch exhausted sessions to another ranked model (default: true)
  exhausted_patterns       message fragments that identify exhausted models
  acp_registry_url         index URL fetched on refresh (default:
                           https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json)
  python_bin               the Python 3.12 or 3.13 the AI permission model
                           installs into (default: python3.13, python3.12, then python3 on this daemon's PATH)
  nvidia_smi_bin           the `nvidia-smi` the AI permission model's hardware
                           probe runs to find a GPU (default: nvidia-smi on this daemon's PATH)
  gh_bin                   the `gh` the GitHub integration runs (default: gh on
                           this daemon's PATH)
  glab_bin                 the `glab` the GitLab integration runs (default: glab
                           on this daemon's PATH)
  [[acp_agents]]           add an ACP command with a stable `id` and `command` array

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
    ariadne_daemon::resource::raise_open_file_limit();

    let store = ariadne_store::Store::open(&config.db_path)
        .await
        .with_context(|| format!("opening database {}", config.db_path.display()))?;

    // The daemon's own `PATH`, which is the one its agents are started from:
    // a service carries the `PATH` its service file was written with, not the
    // user's.
    let path = std::env::var_os("PATH").unwrap_or_default();
    let agent_registry = ariadne_daemon::acp_discovery::AgentRegistry::new(
        &config.acp_agents,
        config.root.clone(),
        store.clone(),
        &path,
    );
    // Installed before anything writes, so no state change goes unannounced.
    let events = ariadne_daemon::bus::start(store.clone());

    let unix_listener = bind_unix_socket(&config).await?;
    std::fs::write(&config.pid_file, std::process::id().to_string())
        .with_context(|| format!("writing {}", config.pid_file.display()))?;

    let config = std::sync::Arc::new(config);
    agent_registry.discover().await;
    let ai_permissions = ariadne_daemon::ai_permissions::AiPermissions::new(
        store.clone(),
        events.clone(),
        &config,
        ariadne_daemon::timeouts::Timeouts::default(),
    );
    ai_permissions.ensure_device().await;
    let failure_diagnosis = ariadne_daemon::failure_diagnosis::FailureDiagnosis::new(
        config.ai_failure_diagnosis,
        ai_permissions.clone(),
        store.clone(),
        ariadne_daemon::timeouts::Timeouts::default().failure_diagnosis_decision,
    );
    let launcher = std::sync::Arc::new(ariadne_daemon::launcher::Launcher {
        cfg: config.clone(),
        store: store.clone(),
        git: ariadne_daemon::gitwt::GitManager,
        acp: ariadne_daemon::acp::AcpRuntime::new(store.clone())
            .with_exhausted_patterns(config.exhausted_patterns.clone())
            .with_ai_permissions(ai_permissions.clone())
            .with_failure_diagnosis(failure_diagnosis.clone()),
        registry: agent_registry.clone(),
        branches: ariadne_daemon::branch::BranchWatchers::new(events.clone()),
    });
    // The watches are the process's own: whatever was in flight when the last
    // daemon stopped is picked up again here.
    if let Err(e) = launcher.watch_task_branches().await {
        warn!(error = %e, "cannot follow the branches of the tasks already in flight");
    }
    // A remote that changed while no daemon ran is read once, off the request
    // path: a forge CLI asked about an unknown host may take its time (025).
    tokio::spawn({
        let (store, config) = (store.clone(), config.clone());
        async move { ariadne_daemon::forge::detect_all(&store, &config).await }
    });

    ariadne_daemon::checkpoint::start(
        store.clone(),
        ariadne_daemon::timeouts::Timeouts::default().checkpoint,
    );
    let forge_poll = ariadne_daemon::forge::poll::start(
        store.clone(),
        config.clone(),
        &events,
        ariadne_daemon::timeouts::Timeouts::default().forge_poll,
        ariadne_daemon::timeouts::Timeouts::default().forge_details,
    );
    let webhook_listen =
        ariadne_daemon::webhooks::WebhookListen::bind(&config, store.clone(), forge_poll.clone())
            .await
            .context("binding webhook listener")?;
    info!(address = %webhook_listen.address(), "webhook ingress ready");
    let tunnel = ariadne_daemon::forge::tunnel::Tunnel::new(
        store.clone(),
        config.clone(),
        events.clone(),
        &forge_poll,
        ariadne_daemon::timeouts::Timeouts::default(),
    );
    tunnel.start(&webhook_listen);
    let sched_tx = ariadne_daemon::scheduler::start(
        store.clone(),
        launcher.clone(),
        config.prevent_sleep,
        ariadne_daemon::timeouts::Timeouts::default(),
    );
    // A changed pull request is what starts, tells and ends its session.
    forge_poll.connect_scheduler(sched_tx.clone());
    let outside_sessions = ariadne_daemon::acp_sessions::OutsideSessions::from_env();
    // The first listing then finds the snapshot taken, or being taken, rather
    // than asking every agent itself.
    outside_sessions.warm(&agent_registry).await;
    let state = AppState {
        forge_poll,
        store,
        started_at: Instant::now(),
        started_at_utc: chrono::Utc::now(),
        launcher,
        sched_tx: Some(sched_tx),
        events,
        logs,
        agent_registry,
        outside_sessions,
        ai_permissions,
        tunnel: tunnel.clone(),
    };
    let ai_permissions_shutdown = state.ai_permissions.clone();
    let app = http::router(state);

    // The public ingress stops when shutdown starts: an open event stream
    // can hold the HTTP drain below for as long as its client stays.
    tokio::spawn(async move {
        shutdown_signal().await;
        tunnel.shutdown().await;
        drop(webhook_listen);
        info!("webhook ingress stopped");
    });
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
    failure_diagnosis.shutdown().await;
    ai_permissions_shutdown.shutdown().await;
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
