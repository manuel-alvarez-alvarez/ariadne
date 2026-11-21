//! Restore ACP wire state from durable daemon inputs and relay output.
use anyhow::{Context, Result};
use ariadne_store::Store;
use serde_json::Value;
use std::collections::HashMap;

/// A daemon request together with the agent response, if one exists.
#[derive(Debug, Clone)]
pub struct RecoveredRequest {
    pub input_seq: i64,
    pub message: Value,
    pub response: Option<Value>,
    pub response_processed: bool,
    pub response_seq: Option<u64>,
}

/// The information a new daemon needs before it starts dispatching relay output.
#[derive(Debug, Default, Clone)]
pub struct RecoveryHistory {
    pub requests: Vec<RecoveredRequest>,
    pub answered_permissions: HashMap<String, Value>,
    pub last_input_seq: i64,
    pub outputs: Vec<ariadne_store::RelayOutput>,
}
impl RecoveryHistory {
    /// Read one launch without changing its relay or accepting more input.
    pub async fn load(store: &Store, session: &str, launch: &str) -> Result<Self> {
        let outputs = store.relay_outputs(session, launch).await?;
        let mut responses = HashMap::new();
        for output in &outputs {
            let frame: Value =
                serde_json::from_str(&output.frame).context("decoding retained relay output")?;
            if frame["stream"] != "stdout" {
                continue;
            }
            let message: Value = serde_json::from_str(
                frame["data"]
                    .as_str()
                    .context("relay stdout frame has no data")?,
            )
            .context("decoding retained ACP message")?;
            if message.get("method").is_none()
                && let Some(id) = message.get("id")
            {
                responses.insert(
                    id.to_string(),
                    (message, output.processed, u64::try_from(output.seq)?),
                );
            }
        }
        let mut history = Self {
            outputs,
            ..Self::default()
        };
        for input in store.relay_inputs_after(session, launch, 0).await? {
            history.last_input_seq = input.seq;
            let message: Value =
                serde_json::from_str(&input.data).context("decoding retained ACP input")?;
            let Some(id) = message.get("id") else {
                continue;
            };
            if message.get("method").is_some() {
                let response = responses.remove(&id.to_string());
                history.requests.push(RecoveredRequest {
                    input_seq: input.seq,
                    message,
                    response_processed: response
                        .as_ref()
                        .is_some_and(|(_, processed, _)| *processed),
                    response_seq: response.as_ref().map(|(_, _, seq)| *seq),
                    response: response.map(|(message, _, _)| message),
                });
            } else if let Some(result) = message.get("result") {
                history
                    .answered_permissions
                    .insert(id.to_string(), result.clone());
            }
        }
        Ok(history)
    }
    /// Last completed turn before the prompt that still needs processing.
    pub(crate) fn replay_after(&self) -> u64 {
        let pending = self.pending_prompt();
        self.requests
            .iter()
            .filter(|request| {
                request.message["method"] == "session/prompt"
                    && pending.is_none_or(|pending| request.input_seq < pending.input_seq)
            })
            .filter_map(|request| request.response_seq)
            .max()
            .unwrap_or(0)
    }

    /// A prompt whose response still needs runtime processing.
    pub fn pending_prompt(&self) -> Option<&RecoveredRequest> {
        self.requests.iter().rev().find(|request| {
            request.message["method"] == "session/prompt"
                && (request.response.is_none() || !request.response_processed)
        })
    }
}

/// Stable identity for effects derived from one ACP output frame.
#[derive(Clone)]
pub(crate) struct Effect {
    pub(crate) seq: u64,
    pub(crate) source: String,
    pub(crate) ordinal: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Match SDK callbacks to durable relay frames without changing ACP payloads.
pub(crate) struct RelayEffects {
    store: Store,
    session: String,
    launch: String,
    notifications: std::sync::Mutex<HashMap<String, std::collections::VecDeque<Effect>>>,
    permission_replies: std::sync::Mutex<HashMap<String, Effect>>,
    responses: std::sync::Mutex<HashMap<String, Effect>>,
}
impl RelayEffects {
    pub(crate) fn new(store: Store, session: String, launch: String) -> Self {
        Self {
            store,
            session,
            launch,
            notifications: Default::default(),
            permission_replies: Default::default(),
            responses: Default::default(),
        }
    }
    pub(crate) fn track(&self, frame: &Value, message: &Value) -> Result<()> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            if let Some(id) = message.get("id") {
                let seq = frame["seq"]
                    .as_u64()
                    .context("relay frame has no sequence number")?;
                self.responses.lock().expect("relay responses").insert(
                    id.to_string(),
                    Effect {
                        seq,
                        source: format!("output:{seq}"),
                        ordinal: Default::default(),
                    },
                );
            }
            return Ok(());
        };
        let seq = frame["seq"]
            .as_u64()
            .context("relay frame has no sequence number")?;
        let effect = Effect {
            seq,
            source: format!("output:{seq}"),
            ordinal: Default::default(),
        };
        let key = format!("{method}:{}", message.get("params").unwrap_or(&Value::Null));
        self.notifications
            .lock()
            .expect("relay effect queue")
            .entry(key)
            .or_default()
            .push_back(effect.clone());
        if method == "session/request_permission"
            && let Some(id) = message.get("id")
        {
            self.permission_replies
                .lock()
                .expect("relay permission replies")
                .insert(id.to_string(), effect);
        }
        Ok(())
    }
    pub(crate) fn take(&self, method: &str, params: &Value) -> Option<Effect> {
        let key = format!("{method}:{params}");
        let mut notifications = self.notifications.lock().expect("relay effect queue");
        let queue = notifications.get_mut(&key)?;
        let effect = queue.pop_front();
        if queue.is_empty() {
            notifications.remove(&key);
        }
        effect
    }
    pub(crate) fn response(&self, id: &Value) -> Option<Effect> {
        self.responses
            .lock()
            .expect("relay responses")
            .remove(&id.to_string())
    }
    pub(crate) async fn processed(&self, seq: u64) -> Result<()> {
        self.store
            .mark_relay_output_processed(&self.session, &self.launch, i64::try_from(seq)?)
            .await?;
        Ok(())
    }
    pub(crate) async fn reply_sent(&self, id: &Value) -> Result<()> {
        let effect = self
            .permission_replies
            .lock()
            .expect("relay permission replies")
            .remove(&id.to_string());
        if let Some(effect) = effect {
            self.processed(effect.seq).await?;
        }
        Ok(())
    }
}
