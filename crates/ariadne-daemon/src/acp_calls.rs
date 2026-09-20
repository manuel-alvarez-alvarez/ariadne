//! The ACP methods the daemon calls, as the SDK's connection sends them.
//!
//! Each one carries its params as JSON and takes its response as JSON. The
//! params are built from the SDK's typed `schema::v1` requests (`acp::to_params`),
//! so what goes out is checked; the response comes back raw on purpose.
//!
//! Raw responses are what let the daemon stay lenient about what an agent
//! answers with. `find_config_option` reads an agent's model and effort
//! options out of `configOptions` by trying a category and then the names
//! older adapters used, because the three agent CLIs spell the same option
//! differently. Typing that would turn a mismatch into a session quietly
//! running unpinned, so the catalog is read from JSON, as it always has been.

use agent_client_protocol::{Agent, ConnectionTo, JsonRpcRequest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Declare one ACP method: `$name` sends `$method` and takes JSON back.
macro_rules! call {
    ($(#[$doc:meta])* $name:ident => $method:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
        #[request(method = $method, response = Value)]
        #[serde(transparent)]
        pub(crate) struct $name(pub(crate) Value);
    };
}

call!(
    /// Capability negotiation, the first thing on any connection.
    Initialize => "initialize"
);
call!(
    /// A conversation that did not exist before.
    NewSession => "session/new"
);
call!(
    /// A stored conversation, put back at its prompt.
    ResumeSession => "session/resume"
);
call!(
    /// A stored conversation, replayed on the way in.
    LoadSession => "session/load"
);
call!(
    /// A turn: everything the agent does until it stops.
    PromptTurn => "session/prompt"
);
call!(
    /// One session option, which is how a model or an effort is pinned.
    SetConfigOption => "session/set_config_option"
);
call!(
    /// The end of a session a probe opened to read the catalog.
    CloseSession => "session/close"
);
call!(
    /// One page of the sessions an agent has stored.
    ListSessions => "session/list"
);

/// Send one of these calls and wait for the agent's answer.
///
/// The SDK's error carries the JSON-RPC code and message an agent refused
/// with; `method` names which call it refused, which its error does not.
pub(crate) async fn call<R>(cx: &ConnectionTo<Agent>, method: &str, request: R) -> Result<Value>
where
    R: JsonRpcRequest<Response = Value> + Send,
{
    cx.send_request(request)
        .block_task()
        .await
        .with_context(|| format!("ACP {method} failed"))
}
