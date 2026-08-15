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

use ariadne_client::Client;

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
    /// Repo id; only needed when the goal has several repos.
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

fn to_mcp_err(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
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

    async fn post(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        self.client.post_json(path, &body).await.map_err(to_mcp_err)
    }

    async fn submit_verdict(
        &self,
        verdict: &str,
        body: Option<String>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        let value = self
            .post(
                &format!("/v1/tasks/{task}/reviews"),
                serde_json::json!({ "verdict": verdict, "body": body }),
            )
            .await?;
        json_result(value)
    }
}

#[tool_router]
impl AriadneMcp {
    #[tool(description = "Get a task (defaults to your own) with status, branch, reviewers, deps.")]
    async fn get_task(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(req.task_id)?;
        json_result(self.get(&format!("/v1/tasks/{task}")).await?)
    }

    #[tool(description = "Get the goal this session belongs to (title, repos, settings).")]
    async fn get_goal(&self, Parameters(_): Parameters<Empty>) -> Result<CallToolResult, McpError> {
        json_result(self.get(&format!("/v1/goals/{}", self.goal_id)).await?)
    }

    #[tool(
        description = "List conversation messages of a task (defaults to your own; planner without task_id reads the goal thread)."
    )]
    async fn list_messages(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        let path = match req.task_id.clone().or_else(|| self.task_id.clone()) {
            Some(task) => format!("/v1/tasks/{task}/messages?limit=200"),
            None => format!("/v1/goals/{}/messages?limit=200", self.goal_id),
        };
        json_result(self.get(&path).await?)
    }

    #[tool(
        description = "Post a message into a task conversation (or the goal thread for the planner)."
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
            self.post(&path, serde_json::json!({ "body": req.body }))
                .await?,
        )
    }

    // ---- planner ----

    #[tool(description = "Create a task in the goal, assigning the engineer and the reviewers.")]
    async fn create_task(
        &self,
        Parameters(req): Parameters<CreateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = serde_json::json!({
            "title": req.title,
            "description": req.description,
            "engineer_profile": req.engineer_profile,
            "reviewer_profiles": req.reviewer_profiles,
            "depends_on": req.depends_on.unwrap_or_default(),
            "repo_id": req.repo_id,
        });
        json_result(
            self.post(&format!("/v1/goals/{}/tasks", self.goal_id), body)
                .await?,
        )
    }

    #[tool(description = "List all tasks of the goal with their statuses.")]
    async fn list_tasks(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.get(&format!("/v1/tasks?goal={}", self.goal_id))
                .await?,
        )
    }

    #[tool(description = "Edit a task's title/description/reviewers (only before it starts).")]
    async fn update_task(
        &self,
        Parameters(req): Parameters<UpdateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = serde_json::json!({
            "title": req.title,
            "description": req.description,
            "reviewer_profiles": req.reviewer_profiles,
        });
        let value = self
            .client
            .patch_json(&format!("/v1/tasks/{}", req.task_id), &body)
            .await
            .map_err(to_mcp_err)?;
        json_result(value)
    }

    #[tool(description = "Replace the dependency list of a task (only before it starts).")]
    async fn set_dependencies(
        &self,
        Parameters(req): Parameters<SetDependenciesReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = serde_json::json!({ "depends_on": req.depends_on });
        let value = self
            .client
            .patch_json(&format!("/v1/tasks/{}", req.task_id), &body)
            .await
            .map_err(to_mcp_err)?;
        json_result(value)
    }

    #[tool(description = "List available agent profiles (optionally filtered by role).")]
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
        description = "Finalize the plan: the goal becomes active and tasks start executing. Call once the user agrees."
    )]
    async fn finalize_plan(
        &self,
        Parameters(req): Parameters<FinalizePlanReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = serde_json::json!({ "summary": req.summary });
        json_result(
            self.post(&format!("/v1/goals/{}/finalize", self.goal_id), body)
                .await?,
        )
    }

    // ---- engineer ----

    #[tool(description = "Submit your task for review with a summary of what you built.")]
    async fn request_review(
        &self,
        Parameters(req): Parameters<RequestReviewReq>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        // Summary first (reviewers read it), then the status transition.
        self.post(
            &format!("/v1/tasks/{task}/messages"),
            serde_json::json!({ "body": format!("Review requested: {}", req.summary) }),
        )
        .await?;
        let value = self
            .post(
                &format!("/v1/tasks/{task}/transitions"),
                serde_json::json!({ "to": "under_review", "reason": req.summary }),
            )
            .await?;
        json_result(value)
    }

    #[tool(description = "Read the reviews of your task (all rounds).")]
    async fn get_reviews(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        json_result(self.get(&format!("/v1/tasks/{task}/reviews")).await?)
    }

    #[tool(
        description = "Report the completed merge. The daemon verifies the branch is actually merged before accepting."
    )]
    async fn mark_merged(
        &self,
        Parameters(req): Parameters<MarkMergedReq>,
    ) -> Result<CallToolResult, McpError> {
        let task = self.own_task(None)?;
        let value = self
            .post(
                &format!("/v1/tasks/{task}/transitions"),
                serde_json::json!({ "to": "merged", "merge_commit": req.merge_commit }),
            )
            .await?;
        json_result(value)
    }

    // ---- reviewer ----

    #[tool(description = "Get the diff of the task branch against its base branch.")]
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

    #[tool(description = "Approve the change under review for the current round.")]
    async fn approve(
        &self,
        Parameters(req): Parameters<VerdictReq>,
    ) -> Result<CallToolResult, McpError> {
        self.submit_verdict("approve", req.body).await
    }

    #[tool(
        description = "Request changes on the change under review; be concrete about what must change."
    )]
    async fn request_changes(
        &self,
        Parameters(req): Parameters<VerdictReq>,
    ) -> Result<CallToolResult, McpError> {
        self.submit_verdict("request_changes", req.body).await
    }
}

impl ServerHandler for AriadneMcp {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(format!(
            "Ariadne orchestrator tools for this {} session (session {}).",
            match self.role {
                McpRole::Planner => "planner",
                McpRole::Engineer => "engineer",
                McpRole::Reviewer => "reviewer",
            },
            self.session_id
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
        Ok(ListToolsResult::with_all_items(
            self.tool_router
                .list_all()
                .into_iter()
                .filter(|t| allowed.contains(&t.name.as_ref()))
                .collect(),
        ))
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
