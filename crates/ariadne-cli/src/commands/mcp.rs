//! `ariadne mcp serve` — stdio MCP server proxying to the daemon REST API.
//!
//! Spawned by the coding agents (config generated at session spawn). Reads
//! its identity from ARIADNE_* env vars; every REST call carries the session
//! header so the daemon enforces role/task scoping. Tools are additionally
//! filtered by role here so agents never even see out-of-role tools.

mod tools;

use anyhow::{Context as _, Result};
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::*;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServiceExt};

use ariadne_client::{Client, ClientError};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum McpRole {
    Planner,
    Engineer,
    Reviewer,
}

impl McpRole {
    fn as_str(&self) -> &'static str {
        match self {
            McpRole::Planner => "planner",
            McpRole::Engineer => "engineer",
            McpRole::Reviewer => "reviewer",
        }
    }

    /// The tools this role may call — and, since the listing is filtered by
    /// the same list, the only ones it ever sees.
    fn tools(&self) -> &'static [&'static str] {
        match self {
            McpRole::Planner => &[
                "get_task",
                "list_messages",
                "post_message",
                "create_task",
                "update_task",
                "list_profiles",
                "finalize_plan",
            ],
            McpRole::Engineer => &[
                "get_task",
                "list_messages",
                "post_message",
                "request_review",
                "get_reviews",
                "mark_merged",
                "record_pull_request",
                "get_diff",
            ],
            McpRole::Reviewer => &[
                "get_task",
                "list_messages",
                "post_message",
                "get_diff",
                "submit_verdict",
            ],
        }
    }
}

#[derive(Clone)]
pub struct AriadneMcp {
    client: std::sync::Arc<Client>,
    role: McpRole,
    session_id: String,
    goal_id: String,
    task_id: Option<String>,
    tool_router: ToolRouter<Self>,
}

impl AriadneMcp {
    pub fn from_env() -> Result<Self> {
        let session_id =
            std::env::var("ARIADNE_SESSION_ID").context("ARIADNE_SESSION_ID not set")?;
        let role = match std::env::var("ARIADNE_ROLE").unwrap_or_default().as_str() {
            "planner" => McpRole::Planner,
            "engineer" => McpRole::Engineer,
            "reviewer" => McpRole::Reviewer,
            other => anyhow::bail!("unknown ARIADNE_ROLE: {other:?}"),
        };
        Ok(Self {
            client: std::sync::Arc::new(Client::from_env().with_session(session_id.clone())),
            role,
            session_id,
            goal_id: std::env::var("ARIADNE_GOAL_ID").context("ARIADNE_GOAL_ID not set")?,
            task_id: std::env::var("ARIADNE_TASK_ID").ok(),
            tool_router: Self::tool_router(),
        })
    }

    /// An endpoint under the task a tool is about: the one it named, else this
    /// session's own — and a refusal when it has neither.
    fn task_path(&self, named: Option<String>, tail: &str) -> Result<String, McpError> {
        let task = named
            .or_else(|| self.task_id.clone())
            .ok_or_else(|| McpError::invalid_params("no task in scope: pass task_id", None))?;
        Ok(format!("/v1/tasks/{task}{tail}"))
    }

    /// Where a conversation lives: under the task a tool named, else under
    /// this session's own — and, for a planner that has neither, the goal
    /// thread, which is the one conversation with no task behind it.
    fn thread(&self, named: Option<String>, tail: &str) -> String {
        match named.or_else(|| self.task_id.clone()) {
            Some(task) => format!("/v1/tasks/{task}{tail}"),
            None => format!("/v1/goals/{}{tail}", self.goal_id),
        }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, McpError> {
        self.client.get_json(path).await.map_err(to_mcp_err)
    }

    async fn post<B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<serde_json::Value, McpError> {
        self.client.post_json(path, body).await.map_err(to_mcp_err)
    }
}

/// The daemon's refusal, as the agent reads it.
///
/// A 4xx is the agent's own doing — an addressee nobody in the thread answers
/// to, a transition its task cannot make — and the daemon already spelled out
/// what would have worked, so it comes back as bad parameters carrying that
/// sentence rather than as a server failure.
fn to_mcp_err(e: ClientError) -> McpError {
    match &e {
        ClientError::Api { status, .. } if status.is_client_error() => {
            McpError::invalid_params(e.to_string(), None)
        }
        _ => McpError::internal_error(e.to_string(), None),
    }
}

fn json_result(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
    )]))
}

/// The rules that hold whoever is reading them: what Ariadne is reached
/// through, and how a message addresses someone.
///
/// One block, appended to the server's instructions above, which every session
/// of every role receives before its first prompt. It used to be pasted into
/// the three system prompts instead, where it was three copies to keep in step
/// and a profile's own text for a user to edit away.
const SESSION_RULES: &str = r#"Reach Ariadne only through them: every backticked operation in your prompts is one of these tools, never a shell command. `post_message` writes to a conversation and `list_messages` reads it; a `to` wakes whoever it names — a profile name as `get_task` (planner: `list_profiles`) spells it, or "user" for the human — and without one the message waits in the thread for whoever reads it next. Work autonomously; wait for a human only when a message asks. One may attach to this terminal and type follow-ups at any time."#;

impl ServerHandler for AriadneMcp {
    /// The server's own instructions, which every session receives before its
    /// first prompt: what this session is, and the rules that hold for every
    /// role alike.
    ///
    /// The rules used to be a block pasted into all three system prompts.
    /// They are here instead because this is the one text an agent of any
    /// role is handed, and because a profile's prompts are the developer's to
    /// edit: what Ariadne *is* should not be something an edit can delete.
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(format!(
            "Ariadne orchestrator tools for this {} session: session {}, goal {}{}. \
             The tools listed here are the ones your role may call, and every \
             call acts as this session. {SESSION_RULES}",
            self.role.as_str(),
            self.session_id,
            self.goal_id,
            match &self.task_id {
                Some(task) => format!(", task {task}"),
                None => String::new(),
            }
        ));
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let allowed = self.role.tools();
        // SEP-2549 cache hints. Protocol 2026-07-28 requires them on list
        // results, and Claude Code (>= 2.1.x) rejects the whole tool list
        // without them — the tools then silently never load. rmcp fills them
        // for `server/discover` but not here, so mirror its defaults: fresh
        // for 0ms (never cached stale) and private to this session, which is
        // also true — the list is role-filtered. Older clients ignore the
        // extra fields.
        Ok(ListToolsResult::with_all_items(
            self.tool_router
                .list_all()
                .into_iter()
                .filter(|t| allowed.contains(&t.name.as_ref()))
                .collect(),
        )
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if !self.role.tools().contains(&request.name.as_ref()) {
            return Err(McpError::invalid_params(
                format!("tool {} is not available to your role", request.name),
                None,
            ));
        }
        self.tool_router
            .call(ToolCallContext::new(self, request, context))
            .await
    }
}

/// Entry point for `ariadne mcp serve`.
pub async fn serve() -> Result<()> {
    let server = AriadneMcp::from_env()?;
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .context("starting MCP stdio server")?;
    service.waiting().await.context("MCP server terminated")?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const ROLES: [McpRole; 3] = [McpRole::Planner, McpRole::Engineer, McpRole::Reviewer];

    pub(crate) fn server_at(role: McpRole, client: Client) -> AriadneMcp {
        AriadneMcp {
            client: std::sync::Arc::new(client),
            role,
            session_id: "01SESSION".into(),
            goal_id: "01GOAL".into(),
            task_id: Some("01TASK".into()),
            tool_router: AriadneMcp::tool_router(),
        }
    }

    /// The whole tool surface, and which role sees which part of it: a tool a
    /// role is not allowed is a tool it never sees, so an omission here is
    /// invisible from inside the session and an extra one is surface nothing
    /// asks for.
    ///
    /// The engineer owns its task from the first commit to the merge, so the
    /// tools that land one are its own — and the send-back a fourth role once
    /// had is gone with it.
    #[test]
    fn every_role_has_the_tools_its_playbook_names_and_no_others() {
        for (role, tools) in [
            (
                McpRole::Planner,
                &[
                    "get_task",
                    "list_messages",
                    "post_message",
                    "create_task",
                    "update_task",
                    "list_profiles",
                    "finalize_plan",
                ][..],
            ),
            (
                McpRole::Engineer,
                &[
                    "get_task",
                    "list_messages",
                    "post_message",
                    "request_review",
                    "get_reviews",
                    "mark_merged",
                    "record_pull_request",
                    "get_diff",
                ][..],
            ),
            (
                McpRole::Reviewer,
                &[
                    "get_task",
                    "list_messages",
                    "post_message",
                    "get_diff",
                    "submit_verdict",
                ][..],
            ),
        ] {
            assert_eq!(role.tools(), tools, "the tools of the {role:?}");
        }

        /// Every tool the three roles are allowed between them, in one list,
        /// so a tool added or dropped is a line of this file.
        const EVERY_TOOL: &[&str] = &[
            "create_task",
            "finalize_plan",
            "get_diff",
            "get_reviews",
            "get_task",
            "list_messages",
            "list_profiles",
            "mark_merged",
            "post_message",
            "record_pull_request",
            "request_review",
            "submit_verdict",
            "update_task",
        ];
        assert_eq!(distinct_tools(), EVERY_TOOL);
        assert!(
            !distinct_tools().contains(&"return_to_engineer"),
            "the send-back a fourth role once had is gone"
        );
    }

    /// The tools of the three roles together, deduplicated and sorted.
    fn distinct_tools() -> Vec<&'static str> {
        let mut tools: Vec<&str> = ROLES.iter().flat_map(|role| role.tools()).copied().collect();
        tools.sort_unstable();
        tools.dedup();
        tools
    }

    /// Every tool a role is allowed is a tool the router really has, and every
    /// tool the router has is one some role may call: a name that has drifted
    /// from its `#[tool]` is filtered out of the listing and refused when
    /// called, which is invisible until an agent needs it.
    #[test]
    fn every_allowed_tool_is_one_the_router_serves() {
        let mut served: Vec<String> = AriadneMcp::tool_router()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        served.sort();
        assert_eq!(served, distinct_tools());
    }

    /// The rules that hold for every role are the server's instructions, and
    /// every session gets them whatever its role and whatever its profile's
    /// prompts have been edited into.
    #[test]
    fn every_session_is_told_how_ariadne_is_reached() {
        for role in ROLES {
            let mcp = server_at(role.clone(), Client::resolve(Some("http://127.0.0.1:1"), None));
            let instructions = mcp.get_info().instructions.expect("instructions");
            for rule in [
                "Reach Ariadne only through them",
                "`post_message` writes to a conversation",
                "\"user\" for the human",
                "Work autonomously",
            ] {
                assert!(instructions.contains(rule), "{role:?}: {instructions}");
            }
        }
    }

    /// The daemon refuses an addressee with the sentence that says which ones
    /// would have worked; that sentence is the whole value of the failure, so
    /// it has to reach the agent instead of a generic "call failed".
    #[test]
    fn a_refused_addressee_reaches_the_agent_in_the_daemons_words() {
        let refusal =
            "Planner takes no part in this thread; address one of: Engineer, Reviewer, user";
        let err = to_mcp_err(ClientError::Api {
            status: http::StatusCode::BAD_REQUEST,
            code: "bad_request".into(),
            message: refusal.into(),
        });
        assert!(err.message.contains(refusal), "{}", err.message);
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    /// One request as a fake daemon read it.
    #[derive(Clone, Debug)]
    pub(crate) struct Seen {
        pub method: String,
        pub path: String,
        pub body: String,
    }

    /// A daemon that records what reaches it and answers every call with an
    /// empty JSON object: enough to count the requests one tool call makes and
    /// to read what it sent.
    pub(crate) async fn recording_daemon() -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = format!("http://{}", listener.local_addr().expect("addr"));
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut raw = Vec::new();
                let mut buf = [0u8; 1024];
                // Read until the headers are in and then to the end of the
                // body the content-length announces.
                loop {
                    let read = match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    raw.extend_from_slice(&buf[..read]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    let Some(head_end) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let head = &text[..head_end];
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")?
                                .trim()
                                .parse()
                                .ok()
                        })
                        .unwrap_or(0);
                    if text.len() < head_end + 4 + length {
                        continue;
                    }
                    let mut start = head.lines().next().unwrap_or_default().split_whitespace();
                    recorded.lock().expect("lock").push(Seen {
                        method: start.next().unwrap_or_default().to_string(),
                        path: start.next().unwrap_or_default().to_string(),
                        body: text[head_end + 4..].to_string(),
                    });
                    break;
                }
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}",
                    )
                    .await;
            }
        });
        (endpoint, seen)
    }
}
