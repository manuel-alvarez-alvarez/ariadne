//! Sending one ACP call and waiting for the agent's answer.
//!
//! Every request is one of the SDK's typed `schema::v1` requests and comes
//! back as the response that request declares, so the shapes on the wire are
//! the protocol's rather than this crate's idea of them.

use agent_client_protocol::{Agent, ConnectionTo, JsonRpcRequest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `session/prompt`, answered as the JSON the agent sent.
///
/// The one call whose answer is not read through the SDK. What a turn spent
/// is the point of it, and the adapters disagree about where that lives and
/// what it is called: one puts it under `_meta.quota.token_count` with
/// `cachedInputTokens`, another at `usage` with `cachedReadTokens`, and the
/// typed response models the second only behind an unstable feature. Read as
/// JSON, both are found; read through the type, neither is certain to be.
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcRequest)]
#[request(method = "session/prompt", response = Value)]
#[serde(transparent)]
pub(crate) struct PromptTurn(pub(crate) Value);

/// Send `request` and wait for its answer.
///
/// `method` names the call in the error: the SDK's carries the JSON-RPC code
/// and message an agent refused with, which does not say which call it was.
pub(crate) async fn call<R>(
    cx: &ConnectionTo<Agent>,
    method: &str,
    request: R,
) -> Result<R::Response>
where
    R: JsonRpcRequest + Send,
{
    cx.send_request(request)
        .block_task()
        .await
        .with_context(|| format!("ACP {method} failed"))
}
