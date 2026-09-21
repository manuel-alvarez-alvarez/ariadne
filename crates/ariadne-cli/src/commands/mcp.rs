//! `ariadne mcp serve` — stdio MCP server proxying to the daemon REST API.
//!
//! Spawned by the coding agents (config generated at session spawn). Reads
//! its identity from ARIADNE_* env vars; every REST call carries the session
//! header so the daemon enforces seat/task scoping. Tools are additionally
//! filtered by seat here so agents never even see out-of-seat tools.

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
pub(crate) enum McpSeat {
    Orchestrator,
    Author,
    Reviewer,
}

impl McpSeat {
    fn as_str(&self) -> &'static str {
        match self {
            McpSeat::Orchestrator => "orchestrator",
            McpSeat::Author => "author",
            McpSeat::Reviewer => "reviewer",
        }
    }

    /// The tools this seat may call — and, since the listing is filtered by
    /// the same list, the only ones it ever sees.
    fn tools(&self) -> &'static [&'static str] {
        match self {
            McpSeat::Orchestrator => &[
                "get_task",
                "create_task",
                "update_task",
                "list_models",
                "list_skills",
                "finalize_plan",
                "list_tasks",
                "retry_task",
                "cancel_task",
                "complete_goal",
                "send_message",
                "read_messages",
                "save_memory",
                "search_memory",
                "search_code",
                "outline",
                "symbol",
                "path",
                "impact",
                "repo_map",
            ],
            McpSeat::Author => &[
                "get_task",
                "request_review",
                "fail_task",
                "finish_task",
                "record_pull_request",
                "send_message",
                "read_messages",
                "save_memory",
                "search_memory",
                "search_code",
                "outline",
                "symbol",
                "path",
                "impact",
                "repo_map",
            ],
            McpSeat::Reviewer => &[
                "get_task",
                "get_diff",
                "submit_verdict",
                "pick_winner",
                "send_message",
                "read_messages",
                "save_memory",
                "search_memory",
                "search_code",
                "outline",
                "symbol",
                "path",
                "impact",
                "repo_map",
            ],
        }
    }
}

/// The tools of the knowledge base (022): every seat has them, and none has
/// them while the daemon runs with `knowledge_enabled = false`.
const KNOWLEDGE_TOOLS: &[&str] = &[
    "search_code",
    "outline",
    "symbol",
    "path",
    "impact",
    "repo_map",
];

#[derive(Clone)]
pub(crate) struct AriadneMcp {
    client: std::sync::Arc<Client>,
    seat: McpSeat,
    session_id: String,
    goal_id: String,
    task_id: Option<String>,
    /// What the daemon said at launch: off, the knowledge tools are neither
    /// listed nor served.
    knowledge_enabled: bool,
    tool_router: ToolRouter<Self>,
}

impl AriadneMcp {
    pub(crate) fn from_env() -> Result<Self> {
        let session_id =
            std::env::var("ARIADNE_SESSION_ID").context("ARIADNE_SESSION_ID not set")?;
        let seat = match std::env::var("ARIADNE_SEAT").unwrap_or_default().as_str() {
            "orchestrator" => McpSeat::Orchestrator,
            "author" => McpSeat::Author,
            "reviewer" => McpSeat::Reviewer,
            other => anyhow::bail!("unknown ARIADNE_SEAT: {other:?}"),
        };
        Ok(Self {
            client: std::sync::Arc::new(Client::from_env().with_session(session_id.clone())),
            seat,
            session_id,
            goal_id: std::env::var("ARIADNE_GOAL_ID").context("ARIADNE_GOAL_ID not set")?,
            task_id: std::env::var("ARIADNE_TASK_ID").ok(),
            // Only an explicit `false` turns them off: a launch that says
            // nothing is one whose daemon serves them.
            knowledge_enabled: std::env::var("ARIADNE_KNOWLEDGE_ENABLED")
                .map(|value| value != "false")
                .unwrap_or(true),
            tool_router: Self::tool_router(),
        })
    }

    /// Whether this session may call `name`: its seat lists it, and it is
    /// not a knowledge tool of a daemon whose knowledge base is off.
    fn allows(&self, name: &str) -> bool {
        self.seat.tools().contains(&name)
            && (self.knowledge_enabled || !KNOWLEDGE_TOOLS.contains(&name))
    }

    /// The tools this session is listed, which are the ones it may call.
    fn listed_tools(&self) -> Vec<Tool> {
        self.tool_router
            .list_all()
            .into_iter()
            .filter(|t| self.allows(t.name.as_ref()))
            .collect()
    }

    /// An endpoint under the task a tool is about: the one it named, else this
    /// session's own — and a refusal when it has neither.
    fn task_path(&self, named: Option<String>, tail: &str) -> Result<String, McpError> {
        let task = named
            .or_else(|| self.task_id.clone())
            .ok_or_else(|| McpError::invalid_params("no task in scope: pass task_id", None))?;
        Ok(format!("/v1/tasks/{task}{tail}"))
    }

    /// The channel a message tool reads: the task it named, else this
    /// session's own, else the goal.
    ///
    /// A goal has a channel of its own, and it is the orchestrator's inbox —
    /// which is a session with no task, so a refusal for want of one would
    /// leave the seat that reads that channel unable to read it.
    fn channel_path(&self, named: Option<String>, tail: &str) -> String {
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

    /// Resolve the repository for a memory tool, or for an outline: the
    /// task's, else the goal's only one, else a refusal that says to name it.
    async fn memory_repository(&self, named: Option<String>) -> Result<String, McpError> {
        if let Some(repository_id) = named {
            return Ok(repository_id);
        }
        if let Some(task_id) = &self.task_id {
            let task: serde_json::Value = self.get(&format!("/v1/tasks/{task_id}")).await?;
            return task["repo_id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| McpError::internal_error("the task names no repository", None));
        }
        let goal: serde_json::Value = self.get(&format!("/v1/goals/{}", self.goal_id)).await?;
        let repositories = goal["repos"]
            .as_array()
            .ok_or_else(|| McpError::internal_error("the goal names no repository list", None))?;
        match repositories.as_slice() {
            [repository] => repository["id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| McpError::internal_error("the repository has no id", None)),
            _ => Err(McpError::invalid_params(
                "pass repository_id because this goal does not have one repository",
                None,
            )),
        }
    }

    /// Every repository of this session's goal, which is what `repo_map`
    /// reads when the call names none. A map is of one repository, and an
    /// orchestrator exploring a goal wants each of them.
    async fn goal_repositories(&self) -> Result<Vec<String>, McpError> {
        let goal: serde_json::Value = self.get(&format!("/v1/goals/{}", self.goal_id)).await?;
        let repositories: Vec<String> = goal["repos"]
            .as_array()
            .ok_or_else(|| McpError::internal_error("the goal names no repository list", None))?
            .iter()
            .filter_map(|repository| repository["id"].as_str().map(str::to_string))
            .collect();
        match repositories.is_empty() {
            true => Err(McpError::invalid_params(
                "pass repository because this goal names none",
                None,
            )),
            false => Ok(repositories),
        }
    }

    /// This task's repository, and the range of its own change: the base
    /// branch to the task branch. What a reviewer reads when it asks for the
    /// impact of the change it is judging.
    async fn task_diff(&self) -> Result<(String, String), McpError> {
        let Some(task_id) = &self.task_id else {
            return Err(McpError::invalid_params(
                "no task in scope: pass diff as `<base>..<head>`",
                None,
            ));
        };
        let task: serde_json::Value = self.get(&format!("/v1/tasks/{task_id}")).await?;
        let (Some(repository_id), Some(branch)) =
            (task["repo_id"].as_str(), task["branch"].as_str())
        else {
            return Err(McpError::internal_error(
                "the task names no repository and no branch",
                None,
            ));
        };
        let repository: serde_json::Value = self
            .get(&format!("/v1/repositories/{repository_id}"))
            .await?;
        let Some(base) = repository["base_branch"].as_str() else {
            return Err(McpError::internal_error(
                "the repository names no base branch",
                None,
            ));
        };
        Ok((repository_id.to_string(), format!("{base}..{branch}")))
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

/// Whether a session gets an answer to a question: the orchestrator writes a
/// spec with the user, who is there in the console to ask; an author or
/// reviewer works its task alone, and asks only where the task cannot go on
/// without the answer.
///
/// An author or reviewer that does ask reaches the other agents through
/// `send_message` — the seat's own playbook says so, so this rule names no
/// channel and only says when to reach for one.
fn ask_rule(seat: &McpSeat) -> &'static str {
    match seat {
        McpSeat::Orchestrator => {
            "The user answers in your console. Ask in plain turn text, one \
             question at a time. Then wait."
        }
        McpSeat::Author | McpSeat::Reviewer => {
            "Work alone. Ask only where the task cannot go on without an \
             answer."
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
/// for the same reason: it held for all three seats, so all three said it.
/// Asking is the one line that no longer holds for all three, so it alone is
/// picked by seat.
///
/// ASD-STE100 Simplified Technical English is here for a third reason on top
/// of those two: it holds for every word an agent writes, and a profile edit
/// that dropped it would leave that session writing whatever it liked. Five
/// rules are spelled out — the ones a sentence is read against — and the list
/// of what they cover is the list of everything an agent writes, since a rule
/// that named only some of it would read as licence for the rest. Each
/// playbook names the texts of its own seat again, in its own layer.
fn session_rules(seat: &McpSeat) -> String {
    format!(
        r#"Reach Ariadne only through these tools. A backticked name is a tool. {} Find code with `search_code` and `symbol` before you read a file. Call `search_memory` before you repeat a discovery. Run a check in the foreground. Never poll it with a no-op command. Never narrate progress. Take as few turns as you can.

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
        ask_rule(seat)
    )
}

impl ServerHandler for AriadneMcp {
    /// The server's own instructions, which every session receives before its
    /// first prompt: what this session is, and the rules that hold for every
    /// seat alike.
    ///
    /// The rules used to be a block pasted into all three system prompts.
    /// They are here instead because this is the one text an agent of any
    /// seat is handed, and because a profile's prompts are the developer's to
    /// edit: what Ariadne *is* should not be something an edit can delete.
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(format!(
            "Ariadne orchestrator tools for this {} session: session {}, goal {}{}. \
             The tools here are the ones your seat can call. Every call acts as \
             this session. {}",
            self.seat.as_str(),
            self.session_id,
            self.goal_id,
            match &self.task_id {
                Some(task) => format!(", task {task}"),
                None => String::new(),
            },
            session_rules(&self.seat)
        ));
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        // SEP-2549 cache hints. Protocol 2026-07-28 requires them on list
        // results, and Claude Code (>= 2.1.x) rejects the whole tool list
        // without them — the tools then silently never load. rmcp fills them
        // for `server/discover` but not here, so mirror its defaults: fresh
        // for 0ms (never cached stale) and private to this session, which is
        // also true — the list is seat-filtered. Older clients ignore the
        // extra fields.
        Ok(ListToolsResult::with_all_items(self.listed_tools())
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if !self.allows(request.name.as_ref()) {
            return Err(McpError::invalid_params(
                format!("tool {} is not available to your seat", request.name),
                None,
            ));
        }
        self.tool_router
            .call(ToolCallContext::new(self, request, context))
            .await
    }
}

/// Entry point for `ariadne mcp serve`.
pub(crate) async fn serve() -> Result<()> {
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

    const SEATS: [McpSeat; 3] = [McpSeat::Orchestrator, McpSeat::Author, McpSeat::Reviewer];

    pub(crate) fn server_at(seat: McpSeat, client: Client) -> AriadneMcp {
        AriadneMcp {
            client: std::sync::Arc::new(client),
            seat,
            session_id: "01SESSION".into(),
            goal_id: "01GOAL".into(),
            task_id: Some("01TASK".into()),
            knowledge_enabled: true,
            tool_router: AriadneMcp::tool_router(),
        }
    }

    /// The whole tool surface, and which seat sees which part of it: a tool a
    /// seat is not allowed is a tool it never sees, so an omission here is
    /// invisible from inside the session and an extra one is surface nothing
    /// asks for.
    ///
    /// The author owns its task from the first commit to the merge, so the
    /// tools that land one are its own — and the send-back a fourth seat once
    /// had is gone with it. What it has no tools for is reading: the verdicts
    /// reach it in the briefing it is resumed with, and the diff is in the
    /// worktree it is standing in.
    #[test]
    fn every_seat_has_the_tools_its_playbook_names_and_no_others() {
        for (seat, tools) in [
            (
                McpSeat::Orchestrator,
                &[
                    "get_task",
                    "create_task",
                    "update_task",
                    "list_models",
                    "list_skills",
                    "finalize_plan",
                    "list_tasks",
                    "retry_task",
                    "cancel_task",
                    "complete_goal",
                    "send_message",
                    "read_messages",
                    "save_memory",
                    "search_memory",
                    "search_code",
                    "outline",
                    "symbol",
                    "path",
                    "impact",
                    "repo_map",
                ][..],
            ),
            (
                McpSeat::Author,
                &[
                    "get_task",
                    "request_review",
                    "fail_task",
                    "finish_task",
                    "record_pull_request",
                    "send_message",
                    "read_messages",
                    "save_memory",
                    "search_memory",
                    "search_code",
                    "outline",
                    "symbol",
                    "path",
                    "impact",
                    "repo_map",
                ][..],
            ),
            (
                McpSeat::Reviewer,
                &[
                    "get_task",
                    "get_diff",
                    "submit_verdict",
                    "pick_winner",
                    "send_message",
                    "read_messages",
                    "save_memory",
                    "search_memory",
                    "search_code",
                    "outline",
                    "symbol",
                    "path",
                    "impact",
                    "repo_map",
                ][..],
            ),
        ] {
            assert_eq!(seat.tools(), tools, "the tools of the {seat:?}");
        }

        /// Every tool the three seats are allowed between them, in one list,
        /// so a tool added or dropped is a line of this file.
        const EVERY_TOOL: &[&str] = &[
            "cancel_task",
            "complete_goal",
            "create_task",
            "fail_task",
            "finalize_plan",
            "finish_task",
            "get_diff",
            "get_task",
            "impact",
            "list_models",
            "list_skills",
            "list_tasks",
            "outline",
            "path",
            "pick_winner",
            "read_messages",
            "record_pull_request",
            "repo_map",
            "request_review",
            "retry_task",
            "save_memory",
            "search_code",
            "search_memory",
            "send_message",
            "submit_verdict",
            "symbol",
            "update_task",
        ];
        assert_eq!(distinct_tools(), EVERY_TOOL);
        assert!(
            !distinct_tools().contains(&"return_to_author"),
            "the send-back a fourth seat once had is gone"
        );
    }

    /// The tools of the three seats together, deduplicated and sorted.
    fn distinct_tools() -> Vec<&'static str> {
        let mut tools: Vec<&str> = SEATS
            .iter()
            .flat_map(|seat| seat.tools())
            .copied()
            .collect();
        tools.sort_unstable();
        tools.dedup();
        tools
    }

    /// With the knowledge base off, no seat is listed a knowledge tool, and a
    /// call to one is refused by name; every other tool of the seat stays.
    #[test]
    fn the_knowledge_tools_are_not_listed_when_the_knowledge_base_is_off() {
        for seat in SEATS {
            let mut mcp = server_at(
                seat.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            let listed = |mcp: &AriadneMcp| -> Vec<String> {
                mcp.listed_tools()
                    .into_iter()
                    .map(|t| t.name.to_string())
                    .collect()
            };
            let with: Vec<String> = listed(&mcp);
            assert!(
                with.iter().any(|t| t == "search_code"),
                "{seat:?}: {with:?}"
            );
            assert!(with.iter().any(|t| t == "outline"), "{seat:?}: {with:?}");

            mcp.knowledge_enabled = false;
            let without = listed(&mcp);
            for tool in KNOWLEDGE_TOOLS {
                assert!(!without.iter().any(|t| t == tool), "{seat:?}: {without:?}");
                assert!(!mcp.allows(tool), "{seat:?} may still call {tool}");
            }
            let rest: Vec<&String> = with
                .iter()
                .filter(|t| !KNOWLEDGE_TOOLS.contains(&t.as_str()))
                .collect();
            assert_eq!(without.iter().collect::<Vec<_>>(), rest, "{seat:?}");
        }
    }

    /// Every tool a seat is allowed is a tool the router really has, and every
    /// tool the router has is one some seat may call: a name that has drifted
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

    /// The rules that hold for every seat are the server's instructions, and
    /// every session gets them whatever its seat and whatever its profile's
    /// prompts have been edited into: how Ariadne is reached, and how few
    /// turns to spend.
    #[test]
    fn every_session_is_told_how_ariadne_is_reached() {
        for seat in SEATS {
            let mcp = server_at(
                seat.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            let instructions = mcp.get_info().instructions.expect("instructions");
            for rule in [
                "Reach Ariadne only through these tools",
                "Find code with `search_code` and `symbol` before you read a file",
                "Call `search_memory` before you repeat a discovery",
                "as few turns as you can",
                "Write all text in ASD-STE100 Simplified Technical English",
                "Write no more than 20 words in a sentence",
                "commit subjects and bodies",
            ] {
                assert!(instructions.contains(rule), "{seat:?}: {instructions}");
            }
        }
    }

    /// The orchestrator is told the user answers in the console and to ask;
    /// an author or reviewer is told to work alone and ask only where the
    /// task cannot go on without the answer. No seat is told nobody answers.
    #[test]
    fn only_the_orchestrator_is_told_to_ask() {
        let orchestrator = server_at(
            McpSeat::Orchestrator,
            Client::resolve(Some("http://127.0.0.1:1"), None),
        );
        let instructions = orchestrator.get_info().instructions.expect("instructions");
        assert!(instructions.contains("The user answers in your console"));
        assert!(instructions.contains("Ask in plain turn text, one question at a time"));
        assert!(instructions.contains("Then wait"));

        for seat in SEATS {
            let mcp = server_at(
                seat.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            let instructions = mcp.get_info().instructions.expect("instructions");
            assert!(
                !instructions.contains("Nobody answers a question, so do not ask"),
                "{seat:?}: {instructions}"
            );
        }

        for seat in [McpSeat::Author, McpSeat::Reviewer] {
            let mcp = server_at(
                seat.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            let instructions = mcp.get_info().instructions.expect("instructions");
            assert!(
                instructions.contains(
                    "Work alone. Ask only where the task cannot go on \
                     without an answer."
                ),
                "{seat:?}: {instructions}"
            );
        }
    }

    /// And no session is told about a conversation there is not: the thread,
    /// the tools that read and wrote it and the word that addressed a message
    /// are gone, and an instruction that still named one would be an agent
    /// reaching for a tool nothing serves.
    #[test]
    fn no_session_is_told_of_a_conversation() {
        for seat in SEATS {
            let mcp = server_at(
                seat.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            let instructions = mcp.get_info().instructions.expect("instructions");
            for gone in ["thread", "message", "conversation"] {
                assert!(!instructions.contains(gone), "{seat:?}: {instructions}");
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
    /// for every word of every seat, and no playbook can state it for all
    /// three.
    ///
    /// The cap rises to 800 for one more rule: run a check in the
    /// foreground, and never poll a background one with a no-op command.
    /// It holds for every seat the way the others here do, even though
    /// only an author or a reviewer runs a check, because it is the one
    /// place all three are told the same thing at once.
    ///
    /// The cap rises to 850 for one more rule of the same shape: find code
    /// with `search_code` and `symbol` before you read a file. The knowledge
    /// tools are served to every seat (022), and a seat that reads a file it
    /// could have asked for by name pays for the whole file. Each skill
    /// names the tool its own step needs; this is the one line that holds
    /// wherever a seat reaches for a file.
    ///
    /// The cap rises to 900 for the memory rule, which is the same shape
    /// again: call `search_memory` before you repeat a discovery. Every
    /// seat holds the tool (019) and no seat was ever told when to use it,
    /// so the store held nothing. The reading rule is the one line that
    /// holds for every seat alike; when a skill writes a memory is that
    /// skill's own step to say.
    #[test]
    fn the_shared_rules_stay_small() {
        const CAP: usize = 900;
        for seat in SEATS {
            let rules = session_rules(&seat);
            assert!(
                rules.len() <= CAP,
                "the {seat:?} session rules are {} characters, over their {CAP}",
                rules.len()
            );
        }
    }

    /// Every text this server hands an agent is Simplified Technical English,
    /// in the two rules of it a test can read off the text: no sentence runs
    /// past [`ste::MAX_WORDS`], and no sentence uses a word of
    /// [`ste::BANNED`].
    ///
    /// The texts are the instructions of every seat, the rules inside them,
    /// the description of every tool, and every description of the schema an
    /// agent fills a call in against — a doc comment on a request type is one
    /// of those, and reaches the agent as surely as the rest. The rules and
    /// the way they are counted are the store's, where the default prompts
    /// are held to the same two.
    #[test]
    fn every_text_the_server_hands_an_agent_is_simplified_technical_english() {
        use ariadne_store::defaults::ste;

        let mut texts = Vec::new();
        for seat in SEATS {
            texts.push((
                format!("the {} session rules", seat.as_str()),
                session_rules(&seat),
            ));
            let mcp = server_at(
                seat.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            texts.push((
                format!("the {} instructions", seat.as_str()),
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

    /// A failure of the daemon's own — a store that is busy, a git that did
    /// not start — is no wrong argument, and must not reach the agent as one.
    #[test]
    fn a_5xx_reaches_the_agent_as_an_internal_error() {
        let failure = "the knowledge base failed: database is locked";
        let err = to_mcp_err(ClientError::Api {
            status: http::StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error".into(),
            message: failure.into(),
        });
        assert!(err.message.contains(failure), "{}", err.message);
        assert_eq!(err.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
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
    pub(crate) async fn recording_daemon() -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>)
    {
        recording_daemon_answering("{}").await
    }

    /// The same daemon, answering every call with one body of a test's own:
    /// what a tool that reads a listing rather than writing one needs.
    pub(crate) async fn recording_daemon_answering(
        answer: &'static str,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
        recording_daemon_with_answers("200 OK", vec![answer.to_string()], true).await
    }

    /// The same daemon, refusing every call with one status line and one
    /// error envelope.
    pub(crate) async fn refusing_daemon(
        status: &'static str,
        code: &str,
        message: &str,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
        let envelope = serde_json::json!({"error": {"code": code, "message": message}});
        recording_daemon_with_answers(status, vec![envelope.to_string()], true).await
    }

    /// The same daemon, with one answer per request in order.
    pub(crate) async fn recording_daemon_answering_in_order(
        answers: &[&str],
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
        recording_daemon_with_answers(
            "200 OK",
            answers.iter().map(|answer| answer.to_string()).collect(),
            false,
        )
        .await
    }

    async fn recording_daemon_with_answers(
        status: &'static str,
        answers: Vec<String>,
        repeat_last: bool,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = format!("http://{}", listener.local_addr().expect("addr"));
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = seen.clone();
        tokio::spawn(async move {
            let mut answers: std::collections::VecDeque<String> = answers.into();
            let fallback = answers.back().cloned().unwrap_or_else(|| "{}".into());
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
                let answer = match repeat_last {
                    true => fallback.clone(),
                    false => answers.pop_front().unwrap_or_else(|| "{}".into()),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        (endpoint, seen)
    }
}
