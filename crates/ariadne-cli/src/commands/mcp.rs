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
use ariadne_api::tasks::{CreateTaskRequest, TransitionRequest, UpdateTaskRequest};
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
    /// Whom to address, waking them to read it: a profile name as your
    /// briefing and `list_profiles` spell it, or "user" for the human. A task
    /// thread addresses its engineer, its reviewers or the planner; a goal
    /// thread only the planner. Leave it out to address the thread itself.
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
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SetDependenciesReq {
    pub task_id: String,
    /// Full replacement list of dependency task ids.
    pub depends_on: Vec<String>,
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
pub struct VerdictReq {
    /// Reasoning / feedback accompanying the verdict.
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
                "get_goal",
                "list_messages",
                "post_message",
                "create_task",
                "list_tasks",
                "update_task",
                "set_dependencies",
                "list_profiles",
                "finalize_plan",
            ],
            McpRole::Engineer => &[
                "get_task",
                "get_goal",
                "list_messages",
                "post_message",
                "request_review",
                "get_reviews",
                "mark_merged",
            ],
            McpRole::Reviewer => &[
                "get_task",
                "get_goal",
                "list_messages",
                "post_message",
                "get_diff",
                "approve",
                "request_changes",
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

    async fn submit_verdict(
        &self,
        verdict: ReviewVerdict,
        body: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        let value = self
            .post(
                &format!("/v1/tasks/{task}/reviews"),
                &CreateReviewRequest {
                    verdict,
                    body,
                    reviewer_profile: None,
                },
            )
            .await?;
        json_result(value)
    }
}

#[tool_router]
impl AriadneMcp {
    #[tool(description = "Read a task: status, branch, engineer, reviewers, dependencies.")]
    async fn get_task(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(req.task_id)?;
        json_result(self.get(&format!("/v1/tasks/{task}")).await?)
    }

    #[tool(
        description = "Read the goal this session belongs to: title, repositories, task limit, approvals required per task."
    )]
    async fn get_goal(&self, Parameters(_): Parameters<Empty>) -> Result<CallToolResult, McpError> {
        json_result(self.get(&format!("/v1/goals/{}", self.goal_id)).await?)
    }

    #[tool(
        description = "Read a task's conversation, for what the other agents and the user said. Without task_id, a planner reads the goal thread. A message that addressed someone carries a `to`: the profile name it named, or \"user\"."
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
        description = "Write into a task's conversation: the way to reach the other agents and the user. Without task_id, a planner posts to the goal thread. Address one of them with `to` — a profile name (as your briefing and `list_profiles` spell it) or \"user\" — and that recipient is woken and notified; without `to` the message is left to the thread, read whenever someone next looks. A task thread addresses its engineer, its reviewers and the planner; a goal thread addresses the planner. Addressing anyone else is refused, naming who would have worked."
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
            // The planner does not pick an integrator yet; the daemon fills in
            // the built-in one.
            integrator_profile: None,
            reviewer_profiles: req.reviewer_profiles,
            depends_on: req.depends_on.unwrap_or_default(),
        };
        json_result(
            self.post(&format!("/v1/goals/{}/tasks", self.goal_id), &body)
                .await?,
        )
    }

    #[tool(description = "List the goal's tasks with their statuses.")]
    async fn list_tasks(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.get(&format!("/v1/tasks?goal={}", self.goal_id))
                .await?,
        )
    }

    #[tool(
        description = "Edit a task's title, description or reviewers. Only accepted while the task has not started."
    )]
    async fn update_task(
        &self,
        Parameters(req): Parameters<UpdateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = UpdateTaskRequest {
            title: req.title,
            description: req.description,
            reviewer_profiles: req.reviewer_profiles,
            depends_on: None,
        };
        let value = self
            .client
            .patch_json(&format!("/v1/tasks/{}", req.task_id), &body)
            .await
            .map_err(to_mcp_err)?;
        json_result(value)
    }

    #[tool(
        description = "Replace a task's dependency list. Only accepted while the task has not started."
    )]
    async fn set_dependencies(
        &self,
        Parameters(req): Parameters<SetDependenciesReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = UpdateTaskRequest {
            depends_on: Some(req.depends_on),
            ..Default::default()
        };
        let value = self
            .client
            .patch_json(&format!("/v1/tasks/{}", req.task_id), &body)
            .await
            .map_err(to_mcp_err)?;
        json_result(value)
    }

    #[tool(description = "List the agent profiles a task can be assigned to.")]
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

    #[tool(
        description = "Finalize the plan: the goal becomes active and its tasks start executing immediately. Call it only once the user agrees the plan is complete, with no question left open."
    )]
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

    #[tool(
        description = "Submit your task for review: the summary is what the reviewers read first. Call it when the work is complete and verified, and again after each round of requested changes."
    )]
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
        description = "Report the merge you have already made. The daemon checks the branch really is merged into its base before accepting the sha, so report it truthfully."
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

    #[tool(description = "Approve the change under review, as this round's single verdict.")]
    async fn approve(
        &self,
        Parameters(req): Parameters<VerdictReq>,
    ) -> Result<CallToolResult, McpError> {
        self.submit_verdict(ReviewVerdict::Approve, req.body).await
    }

    #[tool(
        description = "Request changes on the change under review, as this round's single verdict: name the files and functions that must change."
    )]
    async fn request_changes(
        &self,
        Parameters(req): Parameters<VerdictReq>,
    ) -> Result<CallToolResult, McpError> {
        self.submit_verdict(ReviewVerdict::RequestChanges, req.body)
            .await
    }
}

impl ServerHandler for AriadneMcp {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(format!(
            "Ariadne orchestrator tools for this {} session: session {}, goal {}{}. \
             The tools listed here are the ones your role may call, and every \
             call acts as this session.",
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
            details: None,
        });
        assert!(err.message.contains(refusal), "{}", err.message);
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }
}
