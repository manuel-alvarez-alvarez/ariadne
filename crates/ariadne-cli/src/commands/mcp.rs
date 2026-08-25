//! `ariadne mcp serve` — stdio MCP server proxying to the daemon REST API.
//!
//! Spawned by the coding agents (config generated at session spawn). Reads
//! its identity from ARIADNE_* env vars; every REST call carries the session
//! header so the daemon enforces role/task scoping. Tools are additionally
//! filtered by role here so agents never even see out-of-role tools.

use anyhow::{Context as _, Result};
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServiceExt, schemars, tool, tool_router};

use ariadne_api::goals::FinalizePlanRequest;
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::reviews::CreateReviewRequest;
use ariadne_api::tasks::{
    CreateTaskRequest, RecordPullRequestRequest, TransitionRequest, UpdateTaskRequest,
};
use ariadne_client::{Client, ClientError};
use ariadne_core::{ReviewVerdict, TaskStatus};

#[derive(Clone, Debug, PartialEq)]
enum McpRole {
    Planner,
    Engineer,
    Reviewer,
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

// ---------- tool parameter types ----------

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct Empty {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct TaskIdOpt {
    /// Task id; defaults to your own task.
    pub task_id: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct PostMessageReq {
    /// Message body for the conversation.
    pub body: String,
    /// Task id; defaults to your own task (planner: goal-level thread).
    pub task_id: Option<String>,
    /// Whom to address, waking them, as your system prompt spells it; leave
    /// it out to address the thread itself.
    pub to: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateTaskReq {
    pub title: String,
    pub description: String,
    /// Engineer profile id or name that will own the task.
    pub engineer_profile: String,
    /// Reviewer profile ids or names, in review order (at least one).
    pub reviewer_profiles: Vec<String>,
    /// Ids of tasks that must merge before this one starts.
    pub depends_on: Option<Vec<String>>,
    /// Repository id; only needed when the goal works in several.
    pub repo_id: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct UpdateTaskReq {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub reviewer_profiles: Option<Vec<String>>,
    /// Full replacement list of the ids of the tasks that must merge first.
    pub depends_on: Option<Vec<String>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListProfilesReq {
    /// Filter: planner | engineer | reviewer
    pub role: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct FinalizePlanReq {
    /// Short summary of the agreed plan.
    pub summary: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct RequestReviewReq {
    /// Summary of what you built, for the reviewers.
    pub summary: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct MarkMergedReq {
    /// The merge commit sha on the base branch.
    pub merge_commit: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct RecordPullRequestReq {
    /// URL of the pull request, as `gh pr create` or `glab mr create`
    /// printed it.
    pub url: String,
}

/// The two verdicts a review round can end in, as the one verdict tool takes
/// them.
#[derive(Clone, Copy, Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    RequestChanges,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SubmitVerdictReq {
    /// approve | request_changes
    pub verdict: Verdict,
    /// The note that goes with an approval; the feedback the engineer is
    /// resumed with on a change request, where it is required.
    pub body: Option<String>,
}

// ---------- helpers ----------

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

/// One message as an agent reads it: the DTO's nested recipient flattened
/// into the `to` that `post_message` takes, so what a listing shows is the
/// word that addresses a reply back.
fn addressed_message(message: &MessageDto) -> serde_json::Value {
    serde_json::json!({
        "id": message.id,
        "task_id": message.task_id,
        "author_role": message.author_role.as_str(),
        "author_session_id": message.author_session_id,
        "to": message.recipient.as_ref().map(super::recipient_label),
        "body": message.body,
        "created_at": message.created_at,
    })
}

/// The review the daemon records for a verdict, refusing a change request
/// with nothing in it: the body is what the engineer is resumed with, so a
/// round that asks for changes and says nothing asks for nothing.
fn review_request(verdict: Verdict, body: Option<String>) -> Result<CreateReviewRequest, McpError> {
    let body = body.filter(|b| !b.trim().is_empty());
    let verdict = match verdict {
        Verdict::Approve => ReviewVerdict::Approve,
        Verdict::RequestChanges if body.is_none() => {
            return Err(McpError::invalid_params(
                "request_changes needs a body: the feedback the engineer is resumed with",
                None,
            ));
        }
        Verdict::RequestChanges => ReviewVerdict::RequestChanges,
    };
    Ok(CreateReviewRequest {
        verdict,
        body,
        reviewer_profile: None,
    })
}

fn json_result(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
    )]))
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

    fn allowed_tools(&self) -> &'static [&'static str] {
        match self.role {
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

    fn own_task(&self, arg: Option<String>) -> Result<String, McpError> {
        arg.or_else(|| self.task_id.clone())
            .ok_or_else(|| McpError::invalid_params("no task in scope: pass task_id", None))
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

#[tool_router]
impl AriadneMcp {
    #[tool(
        description = "Read a task: its status, its branch, its dependencies and the profile names of its engineer, its reviewers and the planner."
    )]
    async fn get_task(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        let id = self.own_task(req.task_id)?;
        json_result(self.get(&format!("/v1/tasks/{id}")).await?)
    }

    #[tool(
        description = "Read a task's conversation, or the goal thread when a planner passes no task_id."
    )]
    async fn list_messages(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        let path = match req.task_id.clone().or_else(|| self.task_id.clone()) {
            Some(task) => format!("/v1/tasks/{task}/messages?limit=200"),
            None => format!("/v1/goals/{}/messages?limit=200", self.goal_id),
        };
        let messages: Vec<MessageDto> = self.get(&path).await?;
        json_result(serde_json::Value::Array(
            messages.iter().map(addressed_message).collect(),
        ))
    }

    #[tool(
        description = "Write a message into a task's conversation, or into the goal thread when a planner passes no task_id."
    )]
    async fn post_message(
        &self,
        Parameters(req): Parameters<PostMessageReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = match req.task_id.clone().or_else(|| self.task_id.clone()) {
            Some(task) => format!("/v1/tasks/{task}/messages"),
            None => format!("/v1/goals/{}/messages", self.goal_id),
        };
        json_result(
            self.post(
                &path,
                &CreateMessageRequest {
                    body: req.body,
                    to: req.to,
                },
            )
            .await?,
        )
    }

    // ---- planner ----

    #[tool(
        description = "Create one task in the goal, owned by one engineer profile and gated by at least one reviewer profile."
    )]
    async fn create_task(
        &self,
        Parameters(req): Parameters<CreateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = CreateTaskRequest {
            title: req.title,
            description: req.description,
            repo_id: req.repo_id,
            engineer_profile: req.engineer_profile,
            reviewer_profiles: req.reviewer_profiles,
            depends_on: req.depends_on.unwrap_or_default(),
        };
        json_result(
            self.post(&format!("/v1/goals/{}/tasks", self.goal_id), &body)
                .await?,
        )
    }

    #[tool(
        description = "Edit a task's title, description, reviewers or dependencies, as long as it has not started."
    )]
    async fn update_task(
        &self,
        Parameters(req): Parameters<UpdateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = UpdateTaskRequest {
            title: req.title,
            description: req.description,
            reviewer_profiles: req.reviewer_profiles,
            depends_on: req.depends_on,
        };
        let value = self
            .client
            .patch_json(&format!("/v1/tasks/{}", req.task_id), &body)
            .await
            .map_err(to_mcp_err)?;
        json_result(value)
    }

    #[tool(
        description = "List the agent profiles a task can be assigned to, each with the name, model and system prompt that say what it is for."
    )]
    async fn list_profiles(
        &self,
        Parameters(req): Parameters<ListProfilesReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = match req.role {
            Some(role) => format!("/v1/profiles?role={role}"),
            None => "/v1/profiles".to_string(),
        };
        json_result(self.get(&path).await?)
    }

    #[tool(description = "Finalize the plan, which makes the goal active and starts its tasks.")]
    async fn finalize_plan(
        &self,
        Parameters(req): Parameters<FinalizePlanReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = FinalizePlanRequest {
            summary: req.summary,
        };
        json_result(
            self.post(&format!("/v1/goals/{}/finalize", self.goal_id), &body)
                .await?,
        )
    }

    // ---- engineer ----

    #[tool(description = "Submit your task for review, with the summary the reviewers read first.")]
    async fn request_review(
        &self,
        Parameters(req): Parameters<RequestReviewReq>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        // Summary first (reviewers read it), then the status transition.
        self.post(
            &format!("/v1/tasks/{task}/messages"),
            &CreateMessageRequest {
                body: format!("Review requested: {}", req.summary),
                to: None,
            },
        )
        .await?;
        let value = self
            .post(
                &format!("/v1/tasks/{task}/transitions"),
                &TransitionRequest {
                    to: TaskStatus::UnderReview,
                    reason: Some(req.summary),
                    merge_commit: None,
                },
            )
            .await?;
        json_result(value)
    }

    #[tool(description = "Read the verdicts and feedback on your task, every round of them.")]
    async fn get_reviews(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        json_result(self.get(&format!("/v1/tasks/{task}/reviews")).await?)
    }

    #[tool(
        description = "Report the sha your branch landed on its base branch as, which ends the task."
    )]
    async fn mark_merged(
        &self,
        Parameters(req): Parameters<MarkMergedReq>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        let value = self
            .post(
                &format!("/v1/tasks/{task}/transitions"),
                &TransitionRequest {
                    to: TaskStatus::Merged,
                    reason: None,
                    merge_commit: Some(req.merge_commit),
                },
            )
            .await?;
        json_result(value)
    }

    #[tool(description = "Report the URL of the pull or merge request you opened for this task.")]
    async fn record_pull_request(
        &self,
        Parameters(req): Parameters<RecordPullRequestReq>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        let value = self
            .post(
                &format!("/v1/tasks/{task}/pull-request"),
                &RecordPullRequestRequest { url: req.url },
            )
            .await?;
        json_result(value)
    }

    // ---- reviewer ----

    #[tool(description = "Read the diff of the branch under review against its base branch.")]
    async fn get_diff(&self, Parameters(_): Parameters<Empty>) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        // Plain-text endpoint: no JSON decoding.
        let diff = self
            .client
            .get_text(&format!("/v1/tasks/{task}/diff"))
            .await
            .map_err(to_mcp_err)?;
        Ok(CallToolResult::success(vec![ContentBlock::text(diff)]))
    }

    #[tool(
        description = "Deliver your verdict on the change under review, approving it or requesting changes with the feedback the engineer is resumed with."
    )]
    async fn submit_verdict(
        &self,
        Parameters(req): Parameters<SubmitVerdictReq>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        let body = review_request(req.verdict, req.body)?;
        json_result(
            self.post(&format!("/v1/tasks/{task}/reviews"), &body)
                .await?,
        )
    }
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
            match self.role {
                McpRole::Planner => "planner",
                McpRole::Engineer => "engineer",
                McpRole::Reviewer => "reviewer",
            },
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
        let allowed = self.allowed_tools();
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
        if !self.allowed_tools().contains(&request.name.as_ref()) {
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
mod tests {
    use super::*;

    use ariadne_api::messages::MessageRecipientDto;
    use ariadne_core::{AuthorRole, RecipientKind};

    fn message(recipient: Option<MessageRecipientDto>) -> MessageDto {
        MessageDto {
            id: "01MSG".into(),
            goal_id: "01GOAL".into(),
            task_id: Some("01TASK".into()),
            author_role: AuthorRole::Reviewer,
            author_session_id: Some("01SESSION".into()),
            recipient,
            body: "rebase first".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    /// An agent reads a thread to know who was asked; the addressee it reads
    /// is spelled the way `post_message`'s `to` would address them back.
    #[test]
    fn a_listed_message_carries_the_word_that_addressed_it() {
        let addressed = addressed_message(&message(Some(MessageRecipientDto {
            kind: RecipientKind::Profile,
            profile_id: Some("01PROF".into()),
            profile_name: Some("Engineer".into()),
        })));
        assert_eq!(addressed["to"], serde_json::json!("Engineer"));
        assert_eq!(addressed["body"], serde_json::json!("rebase first"));
        assert_eq!(addressed["author_role"], serde_json::json!("reviewer"));

        let to_the_thread = addressed_message(&message(None));
        assert_eq!(to_the_thread["to"], serde_json::Value::Null);
    }

    fn server_at(role: McpRole, client: Client) -> AriadneMcp {
        AriadneMcp {
            client: std::sync::Arc::new(client),
            role,
            session_id: "01SESSION".into(),
            goal_id: "01GOAL".into(),
            task_id: Some("01TASK".into()),
            tool_router: AriadneMcp::tool_router(),
        }
    }

    fn server(role: McpRole) -> AriadneMcp {
        server_at(role, Client::from_env())
    }

    /// The whole tool surface, and which role sees which part of it: a tool a
    /// role is not allowed is a tool it never sees, so an omission here is
    /// invisible from inside the session and an extra one is surface nothing
    /// asks for.
    #[test]
    fn every_role_reads_its_task_and_has_the_tools_its_playbook_names() {
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
            let allowed = server(role.clone()).allowed_tools();
            assert_eq!(allowed, tools, "the tools of the {role:?}");
        }

        assert_eq!(distinct_tools(), EVERY_TOOL);
    }

    /// Every tool the three roles are allowed between them, in one list, so a
    /// tool added or dropped is a line of this file.
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

    /// The tools of the three roles together, deduplicated and sorted.
    fn distinct_tools() -> Vec<&'static str> {
        let mut tools: Vec<&str> = [McpRole::Planner, McpRole::Engineer, McpRole::Reviewer]
            .iter()
            .flat_map(|role| server(role.clone()).allowed_tools().iter().copied())
            .collect();
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

    /// The engineer owns its task from the first commit to the merge, so the
    /// tools that land one are its own — and the send-back a fourth role
    /// once had is gone with it.
    #[test]
    fn the_engineer_has_the_tools_that_land_its_task() {
        let engineer = server(McpRole::Engineer).allowed_tools();
        for landing in ["record_pull_request", "mark_merged", "request_review"] {
            assert!(engineer.contains(&landing), "the engineer cannot {landing}");
        }
        for role in [McpRole::Planner, McpRole::Engineer, McpRole::Reviewer] {
            assert!(
                !server(role.clone())
                    .allowed_tools()
                    .contains(&"return_to_engineer"),
                "{role:?} still has return_to_engineer"
            );
        }
    }

    /// One request as a fake daemon read it.
    #[derive(Clone, Debug)]
    struct Seen {
        method: String,
        path: String,
        body: String,
    }

    /// A daemon that records what reaches it and answers every call with an
    /// empty JSON object: enough to count the requests one tool call makes and
    /// to read what it sent.
    async fn recording_daemon() -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
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

    /// Reading a task is one round trip: the daemon names the profiles on it,
    /// so nothing here has to fetch the goal and the profile list to spell
    /// them. An agent reads its task on every wake-up, so each extra call is a
    /// call the whole run pays.
    #[tokio::test]
    async fn reading_a_task_asks_the_daemon_once() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpRole::Engineer,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.get_task(Parameters(TaskIdOpt { task_id: None }))
            .await
            .expect("read the task");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "GET");
        assert_eq!(seen[0].path, "/v1/tasks/01TASK");
    }

    /// One tool for both verdicts writes the review row each of the two wrote:
    /// the same route, the same verdict word, the same body.
    #[tokio::test]
    async fn a_verdict_is_recorded_the_way_each_of_the_two_tools_recorded_it() {
        for (verdict, word) in [
            (Verdict::Approve, "approve"),
            (Verdict::RequestChanges, "request_changes"),
        ] {
            let (endpoint, seen) = recording_daemon().await;
            let mcp = server_at(
                McpRole::Reviewer,
                Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
            );
            mcp.submit_verdict(Parameters(SubmitVerdictReq {
                verdict,
                body: Some("rebase first".into()),
            }))
            .await
            .expect("verdict");

            let seen = seen.lock().expect("lock").clone();
            assert_eq!(seen.len(), 1, "{seen:?}");
            assert_eq!(seen[0].method, "POST");
            assert_eq!(seen[0].path, "/v1/tasks/01TASK/reviews");
            let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
            assert_eq!(sent["verdict"], serde_json::json!(word));
            assert_eq!(sent["body"], serde_json::json!("rebase first"));
        }
    }

    /// The body of a change request is what the engineer is resumed with, so
    /// one with nothing in it is refused here rather than sent: a round that
    /// asks for changes and says nothing asks for nothing.
    #[tokio::test]
    async fn a_change_request_with_nothing_in_it_is_refused_before_it_is_sent() {
        for body in [None, Some(String::new()), Some("  \n ".into())] {
            let err = review_request(Verdict::RequestChanges, body.clone())
                .expect_err("empty change request");
            assert!(err.message.contains("needs a body"), "{}", err.message);
            assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);

            let (endpoint, seen) = recording_daemon().await;
            let mcp = server_at(
                McpRole::Reviewer,
                Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
            );
            mcp.submit_verdict(Parameters(SubmitVerdictReq {
                verdict: Verdict::RequestChanges,
                body,
            }))
            .await
            .expect_err("empty change request");
            assert!(seen.lock().expect("lock").is_empty());
        }

        // An approval carries a note or nothing: it is not what a round is
        // resumed on.
        let approved = review_request(Verdict::Approve, None).expect("approval");
        assert_eq!(approved.verdict, ReviewVerdict::Approve);
        assert!(approved.body.is_none());
    }

    /// The rules that hold for every role are the server's instructions, and
    /// every session gets them whatever its role and whatever its profile's
    /// prompts have been edited into.
    #[test]
    fn every_session_is_told_how_ariadne_is_reached() {
        for role in [McpRole::Planner, McpRole::Engineer, McpRole::Reviewer] {
            let mcp = server_at(
                role.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
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
}
