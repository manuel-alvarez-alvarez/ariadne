//! Durable daemon side of the version 1 relay socket.
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use ariadne_store::Store;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixStream, unix::OwnedWriteHalf};
use tokio::sync::{Mutex, mpsc};

/// A relay socket with durable input numbers and an output inbox.
pub struct RelayLink {
    store: Store,
    session: String,
    launch: String,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    pending: VecDeque<String>,
    received: mpsc::Receiver<Result<Value>>,
    reader: tokio::task::JoinHandle<()>,
    delivered: HashSet<u64>,
}

impl Drop for RelayLink {
    fn drop(&mut self) {
        // Disconnect this daemon without stopping the relay or its child.
        self.reader.abort();
    }
}

impl RelayLink {
    /// Check the version, restore the inbox, and resend unaccepted inputs.
    pub async fn connect(store: Store, home: &Path, session: &str, launch: &str) -> Result<Self> {
        let socket = UnixStream::connect(home.join("relays").join(format!("{session}.sock")))
            .await
            .context("connecting to agent relay")?;
        let (reader, writer) = socket.into_split();
        let mut lines = BufReader::new(reader).lines();
        let hello: Value = serde_json::from_str(
            &lines
                .next_line()
                .await?
                .context("relay closed before greeting")?,
        )?;
        ensure!(
            hello["type"] == "hello" && hello["protocol_version"] == 1,
            "unsupported agent relay protocol: {hello}"
        );
        let accepted = hello["last_input_seq"]
            .as_u64()
            .context("relay greeting has no input acceptance number")?;
        let accepted =
            i64::try_from(accepted).context("relay input number exceeds storage range")?;
        let pending = store.pending_relay_outputs(session, launch).await?.into();
        let writer = Arc::new(Mutex::new(writer));
        write(&writer, &json!({"type":"replay", "after":0})).await?;
        for input in store.relay_inputs_after(session, launch, accepted).await? {
            write(
                &writer,
                &json!({"type":"message", "seq":input.seq, "data":input.data}),
            )
            .await?;
        }
        let (send, received) = mpsc::channel(64);
        let reader = tokio::spawn({
            let (store, session, launch, writer) = (
                store.clone(),
                session.to_string(),
                launch.to_string(),
                writer.clone(),
            );
            async move {
                let result: Result<()> = async {
                    while let Some(line) = lines.next_line().await? {
                        let frame: Value = serde_json::from_str(&line)?;
                        let seq = frame["seq"]
                            .as_u64()
                            .context("relay output has no sequence number")?;
                        ensure!(seq > 0, "relay output number must be positive");
                        ensure!(
                            matches!(frame["type"].as_str(), Some("message" | "exit")),
                            "invalid relay output: {frame}"
                        );
                        let pending = store
                            .save_relay_output(&session, &launch, i64::try_from(seq)?, &line)
                            .await?;
                        write(&writer, &json!({"type":"ack", "seq":seq})).await?;
                        if pending && send.send(Ok(frame)).await.is_err() {
                            return Ok(());
                        }
                    }
                    Ok(())
                }
                .await;
                if let Err(error) = result {
                    let _ = send.send(Err(error)).await;
                }
            }
        });
        Ok(Self {
            store,
            session: session.into(),
            launch: launch.into(),
            writer,
            pending,
            received,
            reader,
            delivered: HashSet::new(),
        })
    }

    /// Receive a durable frame. Cancellation does not consume a partial frame.
    pub async fn receive(&mut self) -> Result<Value> {
        loop {
            let frame = if let Some(frame) = self.pending.pop_front() {
                serde_json::from_str(&frame)?
            } else {
                self.received
                    .recv()
                    .await
                    .context("agent relay closed the socket")??
            };
            let seq = frame["seq"]
                .as_u64()
                .context("stored relay output has no sequence number")?;
            if self.delivered.insert(seq) {
                return Ok(frame);
            }
        }
    }

    /// Confirm that the runtime stored this frame's events and usage.
    pub async fn processed(&mut self, seq: u64) -> Result<()> {
        ensure!(
            self.delivered.contains(&seq),
            "relay output was not delivered on this connection"
        );
        self.store
            .mark_relay_output_processed(&self.session, &self.launch, i64::try_from(seq)?)
            .await?;
        Ok(())
    }

    /// Number and persist an ACP input before writing it to the relay.
    pub async fn send(&mut self, data: &str) -> Result<u64> {
        ensure!(
            !data.contains(['\r', '\n']),
            "relay input must contain one ACP line"
        );
        let seq = self
            .store
            .enqueue_relay_input(&self.session, &self.launch, data)
            .await?;
        write(
            &self.writer,
            &json!({"type":"message", "seq":seq, "data":data}),
        )
        .await?;
        Ok(u64::try_from(seq)?)
    }
}

async fn write(writer: &Mutex<OwnedWriteHalf>, frame: &Value) -> Result<()> {
    let mut line = serde_json::to_vec(frame)?;
    line.push(b'\n');
    writer.lock().await.write_all(&line).await?;
    Ok(())
}
