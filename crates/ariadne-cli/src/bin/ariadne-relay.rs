//! Standalone transport owner. The daemon does not launch this binary yet.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use rustix::process::{Pid, Signal, kill_process_group};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, mpsc};

#[derive(Parser)]
#[command(about = "Hold one ACP agent and replay its output over a Unix socket")]
struct Args {
    #[arg(long)]
    home: Option<PathBuf>,
    #[arg(long)]
    session: String,
    #[arg(long)]
    agent: String,
    #[arg(long)]
    task: String,
    #[arg(required = true, trailing_var_arg = true)]
    command: Vec<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Replay { after: u64 },
    Ack { seq: u64 },
    Message { data: String },
    Stop,
}

struct Journal {
    file: File,
    frames: Vec<Value>,
    ack: u64,
    exit: Option<u64>,
}
impl Journal {
    fn record(&mut self, mut frame: Value) -> Result<()> {
        let seq = self.frames.len() as u64 + 1;
        frame["seq"] = seq.into();
        self.persist(&frame)?;
        if frame["type"] == "exit" {
            self.exit = Some(seq);
        }
        self.frames.push(frame);
        Ok(())
    }
    fn persist(&mut self, frame: &Value) -> Result<()> {
        writeln!(self.file, "{frame}")?;
        self.file.sync_data()?;
        Ok(())
    }
}

// Error paths also stop descendants, including descendants that hold output pipes.
struct Group(Pid);
impl Drop for Group {
    fn drop(&mut self) {
        let _ = kill_process_group(self.0, Signal::KILL);
    }
}
struct SocketFile(PathBuf);
impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = Args::parse();
    match run(args).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("relay: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<()> {
    ensure!(
        !args.session.is_empty()
            && args.session.len() <= 64
            && args
                .session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "session must contain 1–64 ASCII letters, digits, hyphens or underscores"
    );
    let home =
        ariadne_client::endpoint::home(args.home.clone()).context("cannot resolve Ariadne home")?;
    let directory = home.join("relays");
    std::fs::create_dir_all(&directory)?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    let socket = directory.join(format!("{}.sock", args.session));
    let path = directory.join(format!("{}.jsonl", args.session));
    // Never unlink another owner's socket or overwrite an earlier session.
    let listener = UnixListener::bind(&socket)?;
    let _socket = SocketFile(socket);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    let journal = Arc::new(Mutex::new(Journal {
        file,
        frames: Vec::new(),
        ack: 0,
        exit: None,
    }));
    let changed = Arc::new(Notify::new());
    let stop = Arc::new(Notify::new());
    let finished = Arc::new(Notify::new());
    let mut child = tokio::process::Command::new(&args.command[0])
        .args(&args.command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .context("start agent")?;
    let group = Group(
        Pid::from_raw(child.id().context("agent has no PID")? as i32).context("invalid PID")?,
    );
    let (input, mut inputs) = mpsc::channel::<String>(64);
    let mut stdin = child.stdin.take().context("agent stdin")?;
    let writer = tokio::spawn(async move {
        while let Some(data) = inputs.recv().await {
            stdin.write_all(data.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        }
        std::io::Result::Ok(())
    });
    let mut stdout = BufReader::new(child.stdout.take().context("agent stdout")?).lines();
    let mut stderr = BufReader::new(child.stderr.take().context("agent stderr")?).lines();
    let mut out_open = true;
    let mut err_open = true;
    let mut status: Option<std::process::ExitStatus> = None;
    let mut last_activity = chrono::Utc::now().to_rfc3339();
    let mut connection: Option<tokio::task::JoinHandle<()>> = None;
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut exit_recorded = false;
    let stopped = loop {
        if let Some(status) = status
            && !out_open
            && !err_open
            && !exit_recorded
        {
            journal
                .lock()
                .unwrap()
                .record(json!({"type":"exit","code":status.code(),"signal":status.signal()}))?;
            exit_recorded = true;
            changed.notify_one();
        }
        tokio::select! {
            _ = stop.notified() => { break true; }
            _ = finished.notified() => { break false; }
            _ = tokio::signal::ctrl_c() => { break true; }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                // A reconnect takes ownership after the previous handler stops.
                if let Some(previous) = connection.take() {
                    previous.abort();
                    let _ = previous.await;
                }
                {
                    let (journal, changed, stop, finished, input) = (journal.clone(), changed.clone(), stop.clone(), finished.clone(), input.clone());
                    connection = Some(tokio::spawn(async move {
                        let _ = serve(stream, journal, changed, stop, finished, input).await;
                    }));
                }
            }
            line = stdout.next_line(), if out_open => {
                match line? {
                    Some(data) => { journal.lock().unwrap().record(json!({"type":"message","stream":"stdout","data":data}))?; changed.notify_one(); last_activity = chrono::Utc::now().to_rfc3339(); }
                    None => out_open = false,
                }
            }
            line = stderr.next_line(), if err_open => {
                match line? {
                    Some(data) => { journal.lock().unwrap().record(json!({"type":"message","stream":"stderr","data":data}))?; changed.notify_one(); last_activity = chrono::Utc::now().to_rfc3339(); }
                    None => err_open = false,
                }
            }
            result = child.wait(), if status.is_none() => {
                status = Some(result?);
                // A descendant must not retain the session's pipes after agent exit.
                let _ = kill_process_group(group.0, Signal::KILL);
                writer.abort();
                last_activity = chrono::Utc::now().to_rfc3339();
            }
            _ = tick.tick() => {
                let state = if status.is_some() { "exited" } else if connection.as_ref().is_some_and(|c| !c.is_finished()) { "connected" } else { "offline" };
                println!("session={} agent={} task={} state={state} activity={last_activity} pid={} | ariadne attach {}", args.session, args.agent, args.task, group.0.as_raw_nonzero(), args.session);
            }
        }
    };
    drop(group);
    writer.abort();
    if let Some(connection) = connection {
        connection.abort();
    }
    if status.is_none() {
        child.wait().await?;
    }
    if stopped {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

async fn serve(
    stream: UnixStream,
    journal: Arc<Mutex<Journal>>,
    changed: Arc<Notify>,
    stop: Arc<Notify>,
    finished: Arc<Notify>,
    input: mpsc::Sender<String>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let sent = AtomicU64::new(u64::MAX);
    let started = Notify::new();
    let receive = async {
        loop {
            let Some(line) = lines.next_line().await? else {
                return Ok(());
            };
            match serde_json::from_str::<Request>(&line)? {
                Request::Replay { after } => {
                    ensure!(
                        sent.load(Ordering::SeqCst) == u64::MAX,
                        "replay already started"
                    );
                    let journal = journal.lock().unwrap();
                    ensure!(
                        after <= journal.ack,
                        "replay cursor exceeds durable acknowledgment"
                    );
                    sent.store(journal.ack, Ordering::SeqCst);
                    started.notify_one();
                }
                Request::Ack { seq } => {
                    let delivered = sent.load(Ordering::SeqCst);
                    ensure!(
                        delivered != u64::MAX && seq <= delivered,
                        "acknowledgment exceeds delivery"
                    );
                    let mut journal = journal.lock().unwrap();
                    ensure!(seq >= journal.ack, "acknowledgment moved backwards");
                    journal.persist(&json!({"type":"ack","seq":seq}))?;
                    journal.ack = seq;
                    if journal.exit == Some(seq) {
                        finished.notify_one();
                    }
                }
                Request::Message { data } => {
                    ensure!(
                        sent.load(Ordering::SeqCst) != u64::MAX,
                        "start replay first"
                    );
                    ensure!(!data.contains(['\n', '\r']), "message must occupy one line");
                    ensure!(journal.lock().unwrap().exit.is_none(), "agent has exited");
                    // Refuse a saturated input queue so stop and reconnect stay available.
                    input.try_send(data).context("agent stdin unavailable")?;
                }
                Request::Stop => {
                    stop.notify_one();
                    return Ok(());
                }
            }
        }
    };
    let transmit = async {
        started.notified().await;
        let mut cursor = sent.load(Ordering::SeqCst);
        loop {
            // Copy one frame at a time. A blocked peer cannot retain a second journal copy.
            let frame = { journal.lock().unwrap().frames.get(cursor as usize).cloned() };
            if let Some(frame) = frame {
                if let Err(error) = writer.write_all(format!("{frame}\n").as_bytes()).await {
                    return Err(anyhow::Error::from(error));
                }
                cursor += 1;
                sent.store(cursor, Ordering::SeqCst);
            } else {
                changed.notified().await;
            }
        }
    };
    tokio::select! {
        result = receive => result,
        result = transmit => result,
    }
}
