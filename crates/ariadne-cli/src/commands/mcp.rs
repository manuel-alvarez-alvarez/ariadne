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
                "create_task",
                "update_task",
                "list_models",
                "list_profiles",
                "finalize_plan",
            ],
            McpRole::Engineer => &[
                "get_task",
                "request_review",
                "fail_task",
                "mark_merged",
                "record_pull_request",
            ],
            McpRole::Reviewer => &["get_task", "get_diff", "submit_verdict"],
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
/// A 4xx is the agent's own doing — a transition its task cannot make, a
/// reviewer it is not assigned as — and the daemon already spelled out what
/// would have worked, so it comes back as bad parameters carrying that
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

/// Whether a session gets an answer to a question: the planner writes a spec
/// with the user, who is there in the terminal to ask; an engineer or
/// reviewer works its task alone, with nobody there to answer one.
///
/// The planner's line replaced "work alone" whole, sentence for sentence: the
/// user answering stands in place of nobody being there, asking in turn text
/// stands in place of not asking, and waiting stands in place of a human
/// attaching whenever it likes — the planner now waits for that human by
/// design.
fn ask_rule(role: &McpRole) -> &'static str {
    match role {
        McpRole::Planner => {
            "The user answers in your terminal. Ask in plain turn text, one \
             question at a time. Then wait."
        }
        McpRole::Engineer | McpRole::Reviewer => {
            "Work alone. Nobody answers a question, so do not ask. A human \
             can attach at any time."
        }
    }
}

/// The rules that hold whoever is reading them: what Ariadne is reached
/// through, how a session gets a question answered, how sparing to be with
/// turns, and the English every text is written in.
///
/// One block, appended to the server's instructions above, which every session
/// receives before its first prompt. It used to be pasted into the three
/// system prompts instead, where it was three copies to keep in step and a
/// profile's own text for a user to edit away. How little to spend is here
/// for the same reason: it held for all three roles, so all three said it.
/// Asking is the one line that no longer holds for all three, so it alone is
/// picked by role.
///
/// ASD-STE100 Simplified Technical English is here for a third reason on top
/// of those two: it holds for every word an agent writes, and a profile edit
/// that dropped it would leave that session writing whatever it liked. Five
/// rules are spelled out — the ones a sentence is read against — and the list
/// of what they cover is the list of everything an agent writes, since a rule
/// that named only some of it would read as licence for the rest. Each
/// playbook names the texts of its own role again, in its own layer.
fn session_rules(role: &McpRole) -> String {
    format!(
        r#"Reach Ariadne only through these tools. A backticked name is a tool. {} Never narrate progress. Take as few turns as you can.

Write all text in ASD-STE100 Simplified Technical English (STE):
- Write one instruction in one sentence.
- Write an instruction in the imperative.
- Write in the active voice.
- Write no more than 20 words in a sentence.
- Write a sequence of steps as a list.

STE holds for all you write:
- your turn text and your visible reasoning
- task titles and descriptions
- `request_review` summaries, verdicts and `fail_task` reasons
- commit subjects and bodies, and pull request text"#,
        ask_rule(role)
    )
}

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
             The tools here are the ones your role can call. Every call acts as \
             this session. {}",
            self.role.as_str(),
            self.session_id,
            self.goal_id,
            match &self.task_id {
                Some(task) => format!(", task {task}"),
                None => String::new(),
            },
            session_rules(&self.role)
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
    /// had is gone with it. What it has no tools for is reading: the verdicts
    /// reach it in the briefing it is resumed with, and the diff is in the
    /// worktree it is standing in.
    #[test]
    fn every_role_has_the_tools_its_playbook_names_and_no_others() {
        for (role, tools) in [
            (
                McpRole::Planner,
                &[
                    "get_task",
                    "create_task",
                    "update_task",
                    "list_models",
                    "list_profiles",
                    "finalize_plan",
                ][..],
            ),
            (
                McpRole::Engineer,
                &[
                    "get_task",
                    "request_review",
                    "fail_task",
                    "mark_merged",
                    "record_pull_request",
                ][..],
            ),
            (
                McpRole::Reviewer,
                &["get_task", "get_diff", "submit_verdict"][..],
            ),
        ] {
            assert_eq!(role.tools(), tools, "the tools of the {role:?}");
        }

        /// Every tool the three roles are allowed between them, in one list,
        /// so a tool added or dropped is a line of this file.
        const EVERY_TOOL: &[&str] = &[
            "create_task",
            "fail_task",
            "finalize_plan",
            "get_diff",
            "get_task",
            "list_models",
            "list_profiles",
            "mark_merged",
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
    /// prompts have been edited into: how Ariadne is reached, and how few
    /// turns to spend.
    #[test]
    fn every_session_is_told_how_ariadne_is_reached() {
        for role in ROLES {
            let mcp = server_at(
                role.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            let instructions = mcp.get_info().instructions.expect("instructions");
            for rule in [
                "Reach Ariadne only through these tools",
                "as few turns as you can",
                "Write all text in ASD-STE100 Simplified Technical English",
                "Write no more than 20 words in a sentence",
                "commit subjects and bodies",
            ] {
                assert!(instructions.contains(rule), "{role:?}: {instructions}");
            }
        }
    }

    /// The planner is told the user answers in the terminal and to ask; an
    /// engineer or reviewer is told the opposite, word for word as before.
    #[test]
    fn only_the_planner_is_told_to_ask() {
        let planner = server_at(
            McpRole::Planner,
            Client::resolve(Some("http://127.0.0.1:1"), None),
        );
        let instructions = planner.get_info().instructions.expect("instructions");
        assert!(instructions.contains("The user answers in your terminal"));
        assert!(instructions.contains("Ask in plain turn text, one question at a time"));
        assert!(instructions.contains("Then wait"));
        assert!(!instructions.contains("Nobody answers a question, so do not ask"));

        for role in [McpRole::Engineer, McpRole::Reviewer] {
            let mcp = server_at(
                role.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            let instructions = mcp.get_info().instructions.expect("instructions");
            assert!(
                instructions.contains(
                    "Work alone. Nobody answers a question, so do not ask. \
                     A human can attach at any time."
                ),
                "{role:?}: {instructions}"
            );
        }
    }

    /// And no session is told about a conversation there is not: the thread,
    /// the tools that read and wrote it and the word that addressed a message
    /// are gone, and an instruction that still named one would be an agent
    /// reaching for a tool nothing serves.
    #[test]
    fn no_session_is_told_of_a_conversation() {
        for role in ROLES {
            let mcp = server_at(role.clone(), Client::resolve(Some("http://127.0.0.1:1"), None));
            let instructions = mcp.get_info().instructions.expect("instructions");
            for gone in ["thread", "message", "conversation"] {
                assert!(!instructions.contains(gone), "{role:?}: {instructions}");
            }
        }
    }

    /// The rules are read before every first prompt of every session, so they
    /// are kept to the size of the rules themselves: each one a clause, none
    /// of them explained twice.
    ///
    /// The cap was 450 for the four rules that were here before the English
    /// the agents write in was one of them. That rule is five short lines and
    /// the list of what they cover, and 700 is what those fit in: it holds
    /// for every word of every role, and no playbook can state it for all
    /// three.
    #[test]
    fn the_shared_rules_stay_small() {
        const CAP: usize = 700;
        for role in ROLES {
            let rules = session_rules(&role);
            assert!(
                rules.len() <= CAP,
                "the {role:?} session rules are {} characters, over their {CAP}",
                rules.len()
            );
        }
    }

    /// Every text this server hands an agent is Simplified Technical English,
    /// in the two rules of it a test can read off the text: no sentence runs
    /// past [`ste::MAX_WORDS`], and no sentence uses a word of
    /// [`ste::BANNED`].
    ///
    /// The texts are the instructions of every role, the rules inside them,
    /// the description of every tool, and every description of the schema an
    /// agent fills a call in against — a doc comment on a request type is one
    /// of those, and reaches the agent as surely as the rest. The rules and
    /// the way they are counted are the store's, where the default prompts
    /// are held to the same two.
    #[test]
    fn every_text_the_server_hands_an_agent_is_simplified_technical_english() {
        use ariadne_store::defaults::ste;

        let mut texts = Vec::new();
        for role in ROLES {
            texts.push((
                format!("the {} session rules", role.as_str()),
                session_rules(&role),
            ));
            let mcp = server_at(
                role.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            texts.push((
                format!("the {} instructions", role.as_str()),
                mcp.get_info().instructions.expect("instructions"),
            ));
        }
        for tool in AriadneMcp::tool_router().list_all() {
            texts.push((
                format!("the {} description", tool.name),
                tool.description.as_deref().unwrap_or_default().to_string(),
            ));
            let schema = serde_json::to_value(&tool.input_schema).expect("schema");
            for described in descriptions(&schema) {
                texts.push((
                    format!("a description of the {} schema", tool.name),
                    described,
                ));
            }
        }

        for (name, text) in texts {
            for sentence in ste::sentences(&text) {
                let words = sentence.split_whitespace().count();
                assert!(
                    words <= ste::MAX_WORDS,
                    "{name} runs a sentence of {words} words, over {}: {sentence}",
                    ste::MAX_WORDS
                );
            }
            assert_eq!(
                ste::banned_word(&text),
                None,
                "{name} uses a word STE has no room for: {text}"
            );
        }
    }

    /// Every `description` in a JSON schema, at whatever depth it sits: what
    /// the doc comments on the request types become.
    fn descriptions(value: &serde_json::Value) -> Vec<String> {
        match value {
            serde_json::Value::Object(fields) => fields
                .iter()
                .flat_map(|(name, value)| match (name.as_str(), value.as_str()) {
                    ("description", Some(text)) => vec![text.to_string()],
                    _ => descriptions(value),
                })
                .collect(),
            serde_json::Value::Array(items) => items.iter().flat_map(descriptions).collect(),
            _ => Vec::new(),
        }
    }

    /// The daemon refuses a call with the sentence that says what would have
    /// worked; that sentence is the whole value of the failure, so it has to
    /// reach the agent instead of a generic "call failed".
    #[test]
    fn a_refused_call_reaches_the_agent_in_the_daemons_words() {
        let refusal = "only an approved task can be marked merged (task is in_progress)";
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
        recording_daemon_answering("{}").await
    }

    /// The same daemon, answering every call with one body of a test's own:
    /// what a tool that reads a listing rather than writing one needs.
    pub(crate) async fn recording_daemon_answering(
        answer: &'static str,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
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
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        (endpoint, seen)
    }
}
