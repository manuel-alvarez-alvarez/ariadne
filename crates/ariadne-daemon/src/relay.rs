//! Transport owner for `ariadned relay`, independent of the daemon server.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, ensure};
use clap::Args as ClapArgs;
use rustix::process::{Pid, Signal, kill_process_group};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, mpsc, oneshot};

#[derive(ClapArgs)]
pub(super) struct Args {
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
    Message { seq: u64, data: String },
    Stop,
}

struct Journal {
    file: File,
    frames: Vec<Value>,
    ack: u64,
    last_input_seq: u64,
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
struct Group {
    pid: Pid,
    armed: bool,
}
impl Drop for Group {
    fn drop(&mut self) {
        if self.armed {
            let _ = kill_process_group(self.pid, Signal::KILL);
        }
    }
}
struct SocketFile(PathBuf);
impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(super) async fn run(args: Args, home_override: Option<PathBuf>) -> Result<()> {
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
        ariadne_client::endpoint::home(home_override).context("cannot resolve Ariadne home")?;
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
        last_input_seq: 0,
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
    let mut group = Group {
        pid: Pid::from_raw(child.id().context("agent has no PID")? as i32)
            .context("invalid PID")?,
        armed: true,
    };
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
    let mut connection: Option<(oneshot::Sender<()>, tokio::task::JoinHandle<()>)> = None;
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    // A thread of its own writes the status, so a full or closed stdout never stalls the loop.
    let (status_lines, mut status_rx) = mpsc::channel::<String>(1);
    std::thread::spawn(move || {
        while let Some(line) = status_rx.blocking_recv() {
            let _ = writeln!(std::io::stdout(), "{line}");
        }
    });
    tick.tick().await;
    let _ = status_lines.try_send(format!("session={} agent={} task={} state=offline activity={last_activity} pid={} | ariadne attach {}", args.session, args.agent, args.task, group.pid.as_raw_nonzero(), args.session));
    let mut exit_recorded = false;
    let stopped = loop {
        if let Some(status) = status
            && !out_open
            && !err_open
            && !exit_recorded
        {
            // The child is reaped and both pipes are closed. Never signal this PID again.
            group.armed = false;
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
                if let Some((replace, previous)) = connection.take() {
                    let _ = replace.send(());
                    let _ = previous.await;
                }
                {
                    let (journal, changed, stop, finished, input) = (journal.clone(), changed.clone(), stop.clone(), finished.clone(), input.clone());
                    let (replace, replaced) = oneshot::channel();
                    connection = Some((replace, tokio::spawn(async move {
                        let _ = serve(stream, journal, changed, stop, finished, input, replaced).await;
                    })));
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
                if out_open || err_open {
                    let _ = kill_process_group(group.pid, Signal::KILL);
                } else {
                    group.armed = false;
                }
                writer.abort();
                last_activity = chrono::Utc::now().to_rfc3339();
            }
            _ = tick.tick() => {
                let state = if status.is_some() { "exited" } else if connection.as_ref().is_some_and(|(_, c)| !c.is_finished()) { "connected" } else { "offline" };
                let _ = status_lines.try_send(format!("session={} agent={} task={} state={state} activity={last_activity} pid={} | ariadne attach {}", args.session, args.agent, args.task, group.pid.as_raw_nonzero(), args.session));
            }
        }
    };
    drop(group);
    writer.abort();
    if let Some((_, connection)) = connection {
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
    mut replaced: oneshot::Receiver<()>,
) -> Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    // The greeting is connection metadata, not an acknowledged journal record.
    let last_input_seq = journal.lock().unwrap().last_input_seq;
    let hello = format!(
        "{}\n",
        json!({"type":"hello","protocol_version":1,"last_input_seq":last_input_seq})
    );
    writer.write_all(hello.as_bytes()).await?;
    let mut buffer = Vec::new();
    let sent = AtomicU64::new(u64::MAX);
    let started = Notify::new();
    let receive = async {
        let mut draining = false;
        loop {
            if let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                let bytes: Vec<u8> = buffer.drain(..=end).collect();
                let line = std::str::from_utf8(&bytes)?.trim_end_matches(['\n', '\r']);
                match serde_json::from_str::<Request>(line)? {
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
                    Request::Message { seq, data } => {
                        ensure!(
                            sent.load(Ordering::SeqCst) != u64::MAX,
                            "start replay first"
                        );
                        ensure!(!data.contains(['\n', '\r']), "message must occupy one line");
                        let mut journal = journal.lock().unwrap();
                        if seq <= journal.last_input_seq {
                            continue;
                        }
                        ensure!(journal.exit.is_none(), "agent has exited");
                        // Reserve capacity before accepting. Persistence and enqueue cannot be cancelled.
                        let permit = input.try_reserve().context("agent stdin unavailable")?;
                        journal.persist(&json!({"type":"input","seq":seq}))?;
                        journal.last_input_seq = seq;
                        permit.send(data);
                    }
                    Request::Stop => {
                        stop.notify_one();
                        return Ok(());
                    }
                }
                continue;
            }
            if draining {
                // Read the kernel socket directly, including bytes not yet observed by Tokio.
                let mut bytes = [0; 8192];
                match rustix::io::read(reader.as_ref(), &mut bytes) {
                    Ok(0) | Err(rustix::io::Errno::AGAIN) => return Ok(()),
                    Ok(count) => buffer.extend_from_slice(&bytes[..count]),
                    Err(error) => return Err(error.into()),
                }
            } else {
                tokio::select! {
                    count = reader.read_buf(&mut buffer) => {
                        if count? == 0 { return Ok(()); }
                    }
                    _ = &mut replaced => { draining = true; }
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
                    return Err::<(), _>(anyhow::Error::from(error));
                }
                cursor += 1;
                sent.store(cursor, Ordering::SeqCst);
            } else {
                changed.notified().await;
            }
        }
    };
    tokio::pin!(receive);
    tokio::select! {
        result = &mut receive => result,
        _ = transmit => {
            // An output disconnect must not cancel input already in the socket.
            receive.await
        }
    }
}
