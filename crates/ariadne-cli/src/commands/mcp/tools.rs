//! The tools themselves: what each one takes, and the request it makes.
//!
//! The parameter types are the wire contract — `schemars` derives the JSON
//! schema an agent reads from them, and the `#[tool]` descriptions are what it
//! chooses between — so a rename here is a rename an agent sees.
//!
//! Every tool is the same three steps: name the endpoint, send the request,
//! answer with what came back. Only the endpoint differs, and the ones that
//! depend on the session rather than on the arguments go through
//! [`AriadneMcp::task_path`].

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};

use ariadne_api::goals::{CompleteGoalRequest, FinalizePlanRequest};
use ariadne_api::messages::SendMessageRequest;
use ariadne_api::pull_requests::{
    ReplyCommentRequest, ReportPullRequestRequest, ReviewCommentRequest, SubmitReviewRequest,
};
use ariadne_api::sessions::SwitchSessionRequest;
use ariadne_api::skills::{SkillDto, SkillSeat};
use ariadne_api::tasks::{
    AgentAssignment, CompleteStepRequest, CreateTaskRequest, FailStepRequest,
    OpenPullRequestRequest, TransitionRequest, UpdateTaskRequest,
};
use ariadne_core::{Actor, TaskStatus};

use super::{AriadneMcp, json_result, to_mcp_err};

// ---------- tool parameter types ----------

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct Empty {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct TaskIdOpt {
    /// Task id. Omit it for your own task.
    pub task_id: Option<String>,
}

/// One agent an orchestrator staffs on one workflow column.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct AgentReq {
    /// The workflow column this agent works in.
    pub step: String,
    /// What it runs on, `<agent>:<model>` as `list_models` spells it.
    /// The column prefers a rank. Use it unless you have a reason to move.
    pub model: String,
    /// An `efforts[].id` `list_models` lists for that model. Omit it for the
    /// default effort.
    pub effort: Option<String>,
    /// The skills this agent loads. Omit them to use the column's own skills.
    pub skills: Option<Vec<String>>,
    /// What to tell this agent beyond the task. Omit it where the task says
    /// everything.
    pub brief: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct CreateTaskReq {
    pub title: String,
    pub description: String,
    /// The agents that work the workflow columns, one for each column. Use
    /// the column's preferred rank unless a reason calls for another model.
    pub agents: Vec<AgentReq>,
    /// Ids of the tasks that must merge before this one starts.
    pub depends_on: Option<Vec<String>>,
    /// Repository id. Pass it only where the goal works in several.
    pub repo_id: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct UpdateTaskReq {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The agents that work the workflow columns, one for each column. This
    /// list replaces the whole staffing list. Use each preferred rank unless
    /// a reason calls for another model. A model is required, so `default`
    /// is refused as one; `default` as an effort puts the agent back on the
    /// default effort.
    pub agents: Option<Vec<AgentReq>>,
    /// The ids of the tasks that must merge first. This list replaces the
    /// whole list.
    pub depends_on: Option<Vec<String>>,
}

/// The one task a supervising tool acts on, by id.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct TaskId {
    /// Task id, as `list_tasks` gives it.
    pub task_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct SwitchSessionReq {
    /// The session to switch, the `session_id` beside an agent on `get_task`.
    pub session_id: String,
    /// What to run it on, `<agent>:<model>`, from `list_models`. A model is
    /// required, so `default` is refused.
    pub model: String,
    /// An `efforts[].id` `list_models` lists for that model. Omit it for the
    /// default effort.
    pub effort: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct ListModelsReq {
    /// Filter: an `agent_id` as `list_models` gives it.
    pub agent_id: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct FailTaskReq {
    /// Why you cannot do the task as written. Ariadne records it on the
    /// task, and the user reads only this.
    pub reason: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct CompleteStepReq {
    /// The summary the next column agent reads.
    pub reason: String,
    /// The base branch merge sha. Pass it only where the last column needs it.
    pub merge_commit: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct FailStepReq {
    /// The feedback the previous column agent reads.
    pub reason: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct OpenPullRequestReq {
    /// Titled by the repository's own commit conventions.
    pub title: String,
    /// Filled from the repository's own template, or, with none, three
    /// sections: Why, What changed, How to test.
    pub body: String,
    /// Opens the request as a draft.
    #[serde(default)]
    pub draft: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct GetDiffReq {
    /// On a pull request, the sha to read the diff from. Omit it for the
    /// whole change.
    pub since: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct SendMessageReq {
    /// Who to write to: a column id from `get_task`, which names the agent
    /// that staffs it, or `orchestrator`.
    pub to: String,
    /// What it needs from you, whole. Nobody answers it.
    // `message` is taken too: an agent that spells the field that way writes
    // the message it meant to, instead of reading a refusal and spending a
    // turn on the same call again.
    #[serde(alias = "message")]
    pub body: String,
    /// The task it is about. Omit it for your own task.
    pub task_id: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct UserRequestReq {
    pub summary: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct WithdrawUserRequestReq {
    pub request_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct ListCommentsReq {
    /// List only the threads that wait on your answer.
    pub unanswered_only: Option<bool>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct ReplyCommentReq {
    /// The `id` of the comment, as the news or `list_comments` gives it.
    pub comment_id: String,
    /// What you changed, or why the code stays.
    pub body: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct GetCommentReq {
    /// The `id` of the comment, as the news or `list_comments` gives it.
    pub comment_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct ResolveThreadReq {
    /// The `id` of a comment of the thread, as `list_comments` gives it.
    pub comment_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct ReportPullRequestReq {
    /// True once every required approval and check reads green. False when
    /// a later change turns one back.
    pub ready: Option<bool>,
    /// The head sha you read every approval and check against, from your
    /// own last `get_pull_request` call. Required with `ready: true`.
    pub head_sha: Option<String>,
    /// The head sha your posted review is on.
    pub reviewed_sha: Option<String>,
}

/// The two events a review takes. The user gives every approval.
#[derive(Clone, Copy, Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewEvent {
    RequestChanges,
    Comment,
}

impl ReviewEvent {
    fn as_str(self) -> &'static str {
        match self {
            ReviewEvent::RequestChanges => "request_changes",
            ReviewEvent::Comment => "comment",
        }
    }
}

/// How much a finding costs.
#[derive(Clone, Copy, Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) enum Priority {
    P0,
    P1,
    P2,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct ReviewFinding {
    /// The file, from the root of the worktree.
    pub path: String,
    /// The line of the defect in the new version of the file.
    pub line: i64,
    /// A short title of the defect, in a few words.
    pub title: String,
    /// What goes wrong: the input and the failure it causes. Then how to
    /// fix it.
    pub body: String,
    pub priority: Priority,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct SubmitReviewReq {
    /// `request_changes` while a P0 finding is open. Else `comment`.
    pub event: ReviewEvent,
    /// The whole summary of the review as it stands now: the commit range
    /// you reviewed, the state, and each open finding by priority and title.
    /// Ariadne keeps one summary comment and replaces its text with this.
    pub body: String,
    /// One inline comment per new finding, on the line of the defect. Leave
    /// it empty in a round with no new finding.
    #[serde(default)]
    pub comments: Vec<ReviewFinding>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(super) struct ReadMessagesReq {
    /// The task whose channel to read. Omit it for your own task.
    pub task_id: Option<String>,
    /// Read the whole thread. Omit it to read only what is new for you.
    pub all: Option<bool>,
}

// ---------- helpers ----------

/// One agent an orchestrator staffed on a column, as the API takes it.
/// Omitted skills tell the daemon to use the skills the goal copied from
/// that column.
fn assignment(agent: AgentReq) -> AgentAssignment {
    AgentAssignment {
        step: agent.step,
        skills: agent.skills.unwrap_or_default(),
        model: agent.model,
        effort: agent.effort,
        brief: agent.brief,
    }
}

/// The staffing of a task as the API takes it, refused where an agent names
/// `default` as its model: a model is required, and there is nothing to
/// clear one to. The same word as an effort travels as it was written, since
/// the daemon is what knows it runs the model at the CLI's own.
fn staffing(agents: Vec<AgentReq>) -> Result<Vec<AgentAssignment>, McpError> {
    if agents.is_empty() {
        return Err(McpError::invalid_params(
            "a task needs `agents`, one for each column of the workflow",
            None,
        ));
    }
    if let Some(agent) = agents.iter().find(|a| a.model == "default") {
        return Err(McpError::invalid_params(
            format!(
                "`default` is no model for column {} — a model is required, so name one from \
                 `list_models`",
                agent.step
            ),
            None,
        ));
    }
    Ok(agents.into_iter().map(assignment).collect())
}

/// The catalog narrowed to one agent, or all of it, and always to the
/// models an agent can actually be staffed on. Entries pass through as the
/// daemon wrote them: what a model is called and what it can be run at is the
/// daemon's answer, not this file's.
///
/// A model the user turned off is dropped rather than shown as off. The
/// catalog is what an orchestrator sizes from, and an entry it is told about
/// is one it will pin sooner or later — which the daemon then refuses, in the
/// middle of a plan, over a choice nobody could have made differently.
fn of_agent(models: Vec<serde_json::Value>, agent_id: Option<String>) -> Vec<serde_json::Value> {
    models
        .into_iter()
        .filter(|m| m["enabled"] != serde_json::Value::Bool(false))
        .filter(|m| match &agent_id {
            Some(id) => m["agent_id"] == serde_json::Value::String(id.clone()),
            None => true,
        })
        .collect()
}

/// Who a message is for, as an agent spells it: `orchestrator`, or the
/// workflow column of an agent the task staffs. An agent's id is taken too,
/// since `get_task` shows it beside the column.
///
/// The column is the address because it is what an agent reading `get_task`
/// has in front of it: a task staffs one agent per column, so the word names
/// one reader and leaves nothing to work out.
fn addressee(to: &str, agents: &[serde_json::Value]) -> Result<(Actor, Option<String>), McpError> {
    if to.eq_ignore_ascii_case("orchestrator") {
        return Ok((Actor::Orchestrator, None));
    }
    let Some(agent) = agents.iter().find(|a| {
        a["id"] == to
            || a["step"]
                .as_str()
                .is_some_and(|step| step.eq_ignore_ascii_case(to))
    }) else {
        return Err(McpError::invalid_params(
            format!(
                "no agent {to} on this task. Say `orchestrator`, or one of: {}",
                roll_call(agents)
            ),
            None,
        ));
    };
    let Some(id) = agent["id"].as_str() else {
        return Err(McpError::invalid_params(
            format!("agent {to} has no id to write to"),
            None,
        ));
    };
    Ok((Actor::Agent, Some(id.to_string())))
}

/// The addresses that would have worked, each column with the id of the
/// agent that staffs it: an agent reading a refusal picks its reader from
/// this line.
fn roll_call(agents: &[serde_json::Value]) -> String {
    agents
        .iter()
        .filter_map(|a| {
            let id = a["id"].as_str()?;
            let step = a["step"].as_str().unwrap_or("agent");
            Some(format!("{id} ({step})"))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[tool_router(vis = "pub(super)")]
impl AriadneMcp {
    #[tool(
        description = "Ask the user a question that blocks your work. The Needs attention list opens your console. End your turn after this call."
    )]
    async fn request_user_input(
        &self,
        Parameters(req): Parameters<UserRequestReq>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.post(
                &format!("/v1/sessions/{}/agent-requests", self.session_id),
                &serde_json::json!({"summary": req.summary}),
            )
            .await?,
        )
    }

    #[tool(description = "Withdraw one unanswered user request when it no longer applies.")]
    async fn withdraw_user_request(
        &self,
        Parameters(req): Parameters<WithdrawUserRequestReq>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.post(
                &format!(
                    "/v1/sessions/{}/agent-requests/{}",
                    self.session_id, req.request_id
                ),
                &serde_json::json!({}),
            )
            .await?,
        )
    }
    #[tool(
        description = "Read a task: its column, status, branch, dependencies and agents. Each agent gives its column and skills. The goal columns give each rank and gate."
    )]
    async fn get_task(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        self.task_with_steps(req.task_id).await
    }

    #[tool(
        description = "Complete your column. Give the summary the next column agent reads. The last column ends the task. End your turn after this call."
    )]
    async fn complete_step(
        &self,
        Parameters(req): Parameters<CompleteStepReq>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.post(
                &self.task_path(None, "/step/complete")?,
                &CompleteStepRequest {
                    reason: req.reason,
                    merge_commit: req.merge_commit,
                },
            )
            .await?,
        )
    }

    #[tool(
        description = "Fail your column. Give the feedback the previous column agent reads. A fail on the first column fails the task. End your turn after this call."
    )]
    async fn fail_step(
        &self,
        Parameters(req): Parameters<FailStepReq>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.post(
                &self.task_path(None, "/step/fail")?,
                &FailStepRequest { reason: req.reason },
            )
            .await?,
        )
    }

    // ---- orchestrator ----

    #[tool(
        description = "Create one task in the goal. Pass `agents`, one for each column of the workflow. Each column prefers a rank. Use it unless a reason calls for another model."
    )]
    async fn create_task(
        &self,
        Parameters(req): Parameters<CreateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let agents = staffing(req.agents)?;
        let path = format!("/v1/goals/{}/tasks", self.goal()?);
        json_result(
            self.post(
                &path,
                &CreateTaskRequest {
                    title: req.title,
                    description: req.description,
                    repo_id: req.repo_id,
                    agents,
                    depends_on: req.depends_on.unwrap_or_default(),
                },
            )
            .await?,
        )
    }

    #[tool(
        description = "Edit a task that is pending, ready or failed. `agents` replaces the whole staffing, one for each column. Staff every column a failed task lacks before you retry it."
    )]
    async fn update_task(
        &self,
        Parameters(req): Parameters<UpdateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let agents = req.agents.map(staffing).transpose()?;
        let body = UpdateTaskRequest {
            title: req.title,
            description: req.description,
            agents,
            depends_on: req.depends_on,
        };
        let path = format!("/v1/tasks/{}", req.task_id);
        json_result(
            self.client
                .patch_json(&path, &body)
                .await
                .map_err(to_mcp_err)?,
        )
    }

    #[tool(
        description = "List the agents and models a slot can run on. Each entry gives:\n- its `id`, `<agent>:<model>`, and the description its agent gave\n- `efforts`, each an id and what it buys, one `default`\n- `rank`, the standing the user set: `frontier`, `balanced`, `fast`, `local` or `null`, a ladder from `fast` up"
    )]
    async fn list_models(
        &self,
        Parameters(req): Parameters<ListModelsReq>,
    ) -> Result<CallToolResult, McpError> {
        // The catalog is the union and takes no filter, so an agent narrows
        // what it answered rather than what was asked for.
        let models: Vec<serde_json::Value> = self.get("/v1/models").await?;
        json_result(serde_json::Value::Array(of_agent(models, req.agent_id)))
    }

    #[tool(
        description = "List the skills an agent can load. Each entry gives the name and one line saying what that skill is for. Give an agent the skills its work needs and no more."
    )]
    async fn list_skills(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let skills: Vec<SkillDto> = self.get("/v1/skills").await?;
        // The name and the one line about it, and nothing else. The whole
        // `SKILL.md` of every skill is what an orchestrator used to read to
        // staff one agent — tens of thousands of characters per call, none of
        // which it chooses between. The agent staffed on the skill is the one
        // that reads the document.
        json_result(serde_json::Value::Array(
            skills
                .into_iter()
                .filter(|skill| skill.seat == SkillSeat::Task)
                .map(|skill| serde_json::json!({"name": skill.name, "summary": skill.summary}))
                .collect(),
        ))
    }

    #[tool(
        description = "Finalize the plan. This call starts every task of the plan and ends planning."
    )]
    async fn finalize_plan(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/goals/{}/finalize", self.goal()?);
        json_result(self.post(&path, &FinalizePlanRequest {}).await?)
    }

    #[tool(
        description = "List every task of the goal, with each task and agent column. The goal columns give each rank and gate. This shows where the goal stands."
    )]
    async fn list_tasks(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/tasks?goal_id={}", self.goal()?);
        let mut tasks: serde_json::Value = self.get(&path).await?;
        let goal: serde_json::Value = self.get(&format!("/v1/goals/{}", self.goal()?)).await?;
        if let Some(tasks) = tasks.as_array_mut() {
            for task in tasks {
                task["goal_steps"] = goal["steps"].clone();
            }
        }
        json_result(tasks)
    }

    #[tool(
        description = "Start a failed task again, from its first column. Rewrite it with `update_task` first where it failed on how it was written. A task with a column nobody staffs is refused: staff it first. A task with an unfinished dependency waits for it; retry it with that dependency, not cancel and recreate."
    )]
    async fn retry_task(
        &self,
        Parameters(req): Parameters<TaskId>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/tasks/{}/retry", req.task_id);
        json_result(self.post(&path, &serde_json::json!({})).await?)
    }

    #[tool(
        description = "Give a task up for good. Use it where the goal no longer needs the task, or where nothing you can rewrite would make it work."
    )]
    async fn cancel_task(
        &self,
        Parameters(req): Parameters<TaskId>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/tasks/{}/cancel", req.task_id);
        json_result(self.post(&path, &serde_json::json!({})).await?)
    }

    #[tool(
        description = "Switch a session to the model or agent you give. The new session is briefed with a handoff of the old conversation. A model is required, so `default` is refused."
    )]
    async fn switch_session(
        &self,
        Parameters(req): Parameters<SwitchSessionReq>,
    ) -> Result<CallToolResult, McpError> {
        if req.model == "default" {
            return Err(McpError::invalid_params(
                "`default` is no model — a model is required, so name one from \
                 `list_models`",
                None,
            ));
        }
        let path = format!("/v1/sessions/{}/switch", req.session_id);
        json_result(
            self.post(
                &path,
                &SwitchSessionRequest {
                    model: req.model,
                    effort: req.effort,
                },
            )
            .await?,
        )
    }

    #[tool(
        description = "End the goal. Call it once every task is finished or cancelled and the goal is met. Ariadne refuses it while any task is still going."
    )]
    async fn complete_goal(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/goals/{}/complete", self.goal()?);
        json_result(self.post(&path, &CompleteGoalRequest {}).await?)
    }

    // ---- agent ----

    #[tool(
        description = "Give the task up, because you cannot do it as written. Ariadne records your reason on the task, and the user reads only that reason."
    )]
    async fn fail_task(
        &self,
        Parameters(req): Parameters<FailTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let reason = req.reason.trim();
        if reason.is_empty() {
            return Err(McpError::invalid_params(
                "fail_task needs a reason: it is all the user is told about the task",
                None,
            ));
        }
        json_result(
            self.transition(TaskStatus::Failed, Some(reason.to_string()))
                .await?,
        )
    }

    #[tool(
        description = "Open the pull or merge request your task lands by. Ariadne runs the forge CLI and answers the URL. A second call on the same task answers that URL again and opens nothing."
    )]
    async fn open_pull_request(
        &self,
        Parameters(req): Parameters<OpenPullRequestReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.task_path(None, "/pull-request")?;
        json_result(
            self.post(
                &path,
                &OpenPullRequestRequest {
                    title: req.title,
                    body: req.body,
                    draft: req.draft,
                },
            )
            .await?,
        )
    }

    #[tool(
        description = "Read the diff of the task branch against its base branch. On a pull request, pass `since` to read only the commits after that sha."
    )]
    async fn get_diff(
        &self,
        Parameters(req): Parameters<GetDiffReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.pull_request_id {
            Some(_) => {
                let mut path = self.pull_request_path("/diff").await?;
                if let Some(since) = req.since {
                    path.push_str(&format!("?since={since}"));
                }
                path
            }
            None => self.task_path(None, "/diff")?,
        };
        // Plain-text endpoint: no JSON decoding.
        let diff = self.client.get_text(&path).await.map_err(to_mcp_err)?;
        Ok(CallToolResult::success(vec![ContentBlock::text(diff)]))
    }

    // ---- the request a task opened ----

    #[tool(
        description = "Read your pull request off the forge now: its description, state, branches, checks and failed checks, and whether the head is behind its base. It also gives your worktree, the repository path and your login."
    )]
    async fn get_pull_request(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let mut pull: serde_json::Value = self.get(&self.pull_request_path("").await?).await?;
        let repository: serde_json::Value = self
            .get(&format!(
                "/v1/repositories/{}",
                pull["repository_id"].as_str().unwrap_or_default()
            ))
            .await?;
        let session: serde_json::Value = self
            .get(&format!("/v1/sessions/{}", self.session_id))
            .await?;
        pull["worktree_path"] = session["worktree_path"].clone();
        pull["repository_path"] = repository["path"].clone();
        pull["login"] = repository["forge"]["login"].clone();
        json_result(pull)
    }

    #[tool(
        description = "List the comments of your pull request, read off the forge now, with every field. Set `unanswered_only` for the threads that wait on your answer."
    )]
    async fn list_comments(
        &self,
        Parameters(req): Parameters<ListCommentsReq>,
    ) -> Result<CallToolResult, McpError> {
        let mut path = self.pull_request_path("/comments").await?;
        if req.unanswered_only.unwrap_or(false) {
            path.push_str("?unanswered_only=true");
        }
        json_result(self.get::<serde_json::Value>(&path).await?)
    }

    #[tool(
        description = "Read one comment of your pull request off the forge now, with every field."
    )]
    async fn get_comment(
        &self,
        Parameters(req): Parameters<GetCommentReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self
            .pull_request_path(&format!("/comments/{}", req.comment_id))
            .await?;
        json_result(self.get::<serde_json::Value>(&path).await?)
    }

    #[tool(
        description = "Reply once to one comment of your pull request. Ariadne posts the reply on the forge."
    )]
    async fn reply_comment(
        &self,
        Parameters(req): Parameters<ReplyCommentReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = req.body.trim();
        if body.is_empty() {
            return Err(McpError::invalid_params("a reply needs a body", None));
        }
        let path = self
            .pull_request_path(&format!("/comments/{}/reply", req.comment_id))
            .await?;
        json_result(
            self.post(
                &path,
                &ReplyCommentRequest {
                    body: body.to_string(),
                },
            )
            .await?,
        )
    }

    #[tool(
        description = "Resolve the thread of one of your own review comments once a push fixed it. Ariadne resolves it on the forge. A thread somebody else opened is refused: its author resolves it."
    )]
    async fn resolve_thread(
        &self,
        Parameters(req): Parameters<ResolveThreadReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self
            .pull_request_path(&format!("/comments/{}/resolve", req.comment_id))
            .await?;
        json_result(self.post(&path, &serde_json::json!({})).await?)
    }

    #[tool(
        description = "Report your pull request. Set `ready` to true once every required approval and check reads green. Name `head_sha`, the head you read them on. Set `ready` to false on a change back. Set `reviewed_sha` after you post a review."
    )]
    async fn report_pull_request(
        &self,
        Parameters(req): Parameters<ReportPullRequestReq>,
    ) -> Result<CallToolResult, McpError> {
        if req.ready.is_none() && req.reviewed_sha.is_none() {
            return Err(McpError::invalid_params(
                "report `ready` or `reviewed_sha`",
                None,
            ));
        }
        if req.ready == Some(true) && req.head_sha.is_none() {
            return Err(McpError::invalid_params(
                "`ready: true` names the head you confirmed it on, as `head_sha`",
                None,
            ));
        }
        json_result(
            self.post(
                &self.pull_request_path("/report").await?,
                &ReportPullRequestRequest {
                    ready: req.ready,
                    head_sha: req.head_sha,
                    reviewed_sha: req.reviewed_sha,
                },
            )
            .await?,
        )
    }

    // ---- pull request reviewer ----

    #[tool(
        description = "Post one round of your review in the name of the user. Give each new finding its file, line and priority. The body replaces the text of your one summary comment. Set `event` to `request_changes` while a P0 is open, else to `comment`."
    )]
    async fn submit_review(
        &self,
        Parameters(req): Parameters<SubmitReviewReq>,
    ) -> Result<CallToolResult, McpError> {
        let comments = req
            .comments
            .into_iter()
            .map(|c| ReviewCommentRequest {
                path: c.path,
                line: c.line,
                title: c.title,
                body: c.body,
                priority: format!("{:?}", c.priority),
            })
            .collect();
        json_result(
            self.post(
                &self.pull_request_path("/reviews").await?,
                &SubmitReviewRequest {
                    event: req.event.as_str().to_string(),
                    body: req.body,
                    comments,
                },
            )
            .await?,
        )
    }

    // ---- everyone ----

    #[tool(
        description = "Send one message. Set `to` to a column id from `get_task` or `orchestrator`. Ask or answer questions only. Send no confirmations, thanks, or plans. After a question, end your turn. Do not poll `read_messages`. Ariadne delivers the answer as a new turn."
    )]
    async fn send_message(
        &self,
        Parameters(req): Parameters<SendMessageReq>,
    ) -> Result<CallToolResult, McpError> {
        self.write_message(req.task_id, &req.to, req.body).await
    }

    #[tool(
        description = "Read the messages sent to you that you have not received yet, oldest first. Ariadne hands over each message once. Set `all` to true for the whole thread of the task or the goal."
    )]
    async fn read_messages(
        &self,
        Parameters(req): Parameters<ReadMessagesReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.channel_path(req.task_id, "/messages")?;
        let path = match req.all.unwrap_or(false) {
            true => path,
            // A default read is a delivery: the daemon narrows it to this
            // session's own agent and stamps what it hands over, so nothing
            // reaches an agent twice.
            false => format!("{path}?deliver=true"),
        };
        json_result(self.get::<serde_json::Value>(&path).await?)
    }
}

impl AriadneMcp {
    /// Read one task and append the copied goal columns that explain its
    /// column ids, preferred ranks and gates.
    async fn task_with_steps(&self, named: Option<String>) -> Result<CallToolResult, McpError> {
        let mut task: serde_json::Value = self.get(&self.task_path(named, "")?).await?;
        let goal: serde_json::Value = self
            .get(&format!(
                "/v1/goals/{}",
                task["goal_id"].as_str().unwrap_or_default()
            ))
            .await?;
        task["goal_steps"] = goal["steps"].clone();
        json_result(task)
    }

    /// Write one message about `task_id` — the session's own where it names
    /// none — to whoever `to` spells.
    async fn write_message(
        &self,
        task_id: Option<String>,
        to: &str,
        body: String,
    ) -> Result<CallToolResult, McpError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(McpError::invalid_params("a message needs a body", None));
        }
        let path = self.task_path(task_id.clone(), "/messages")?;
        let (to_actor, to_agent_id) = addressee(to, &self.agents_of(task_id).await?)?;
        let request = SendMessageRequest {
            to_actor,
            to_agent_id,
            body: body.to_string(),
        };
        json_result(self.post(&path, &request).await?)
    }

    /// The agents staffed on a task, as `get_task` lists them.
    async fn agents_of(&self, task_id: Option<String>) -> Result<Vec<serde_json::Value>, McpError> {
        let task: serde_json::Value = self.get(&self.task_path(task_id, "")?).await?;
        Ok(task["agents"].as_array().cloned().unwrap_or_default())
    }

    /// Move this session's own task, which is the only one a column's agent
    /// may move: the one status an agent reaches outside a step call is
    /// `failed`.
    async fn transition(
        &self,
        to: TaskStatus,
        reason: Option<String>,
    ) -> Result<serde_json::Value, McpError> {
        self.post(
            &self.task_path(None, "/transitions")?,
            &TransitionRequest {
                to,
                reason,
                merge_commit: None,
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_client::Client;

    use crate::commands::mcp::McpSeat;
    use crate::commands::mcp::tests::{
        recording_daemon, recording_daemon_answering, recording_daemon_answering_in_turn, server_at,
    };

    /// The request tools of a column's agent reach the routes of the request
    /// its task opened (030): each call finds that request through the
    /// ledger, then reads it with the worktree, repository path and login
    /// beside it, lists its comments, posts one reply, and reports.
    #[tokio::test]
    async fn the_agents_request_tools_call_the_routes_of_the_request_its_task_opened() {
        let found = r#"[{"id":"01PR"}]"#.to_string();
        let row = r#"{"id":"01PR","repository_id":"01R","path":"/repos/widgets","forge":{"login":"me"},"worktree_path":"/wt/task"}"#.to_string();
        let (endpoint, seen) = recording_daemon_answering_in_turn(vec![
            found.clone(),
            row.clone(),
            row.clone(),
            row,
            found.clone(),
            "[]".into(),
            found.clone(),
            "{}".into(),
            found,
            "{}".into(),
        ])
        .await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let read = mcp
            .get_pull_request(Parameters(Empty {}))
            .await
            .expect("read the request");
        let ContentBlock::Text(text) = &read.content[0] else {
            panic!("the request came back as something other than text");
        };
        let read: serde_json::Value = serde_json::from_str(&text.text).expect("json");
        assert_eq!(read["worktree_path"], "/wt/task");
        assert_eq!(read["repository_path"], "/repos/widgets");
        assert_eq!(read["login"], "me");
        mcp.list_comments(Parameters(ListCommentsReq {
            unanswered_only: Some(true),
        }))
        .await
        .expect("list the comments");
        mcp.reply_comment(Parameters(ReplyCommentReq {
            comment_id: "01C".into(),
            body: "Renamed it.".into(),
        }))
        .await
        .expect("reply");
        mcp.report_pull_request(Parameters(ReportPullRequestReq {
            ready: Some(true),
            head_sha: Some("abc".into()),
            reviewed_sha: None,
        }))
        .await
        .expect("report");

        let seen = seen.lock().expect("lock").clone();
        let calls: Vec<(String, String)> = seen
            .iter()
            .map(|s| (s.method.clone(), s.path.clone()))
            .collect();
        let find = (
            "GET".to_string(),
            "/v1/pull-requests?task=01TASK&role=author".to_string(),
        );
        assert_eq!(
            calls,
            [
                find.clone(),
                ("GET".into(), "/v1/pull-requests/01PR".into()),
                ("GET".into(), "/v1/repositories/01R".into()),
                ("GET".into(), "/v1/sessions/01SESSION".into()),
                find.clone(),
                (
                    "GET".into(),
                    "/v1/pull-requests/01PR/comments?unanswered_only=true".into()
                ),
                find.clone(),
                (
                    "POST".into(),
                    "/v1/pull-requests/01PR/comments/01C/reply".into()
                ),
                find,
                ("POST".into(), "/v1/pull-requests/01PR/report".into()),
            ]
        );
        let reply: serde_json::Value = serde_json::from_str(&seen[7].body).expect("json");
        assert_eq!(reply, serde_json::json!({"body": "Renamed it."}));
        let report: serde_json::Value = serde_json::from_str(&seen[9].body).expect("json");
        assert_eq!(
            report,
            serde_json::json!({"ready": true, "head_sha": "abc", "reviewed_sha": null})
        );
    }

    /// An agent whose task opened no request yet is told to open one, and
    /// no request route is called.
    #[tokio::test]
    async fn an_agent_with_no_request_is_told_to_open_one() {
        let (endpoint, seen) = recording_daemon_answering("[]").await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let refused = mcp
            .list_comments(Parameters(ListCommentsReq {
                unanswered_only: None,
            }))
            .await
            .expect_err("no request yet");
        assert!(
            refused.message.contains("`open_pull_request`"),
            "{refused:?}"
        );
        assert_eq!(seen.lock().expect("lock").len(), 1);
    }

    /// The tools of a reviewer pull request session reach the routes of its
    /// own request (029): the diff from a sha, one review with its findings
    /// and their priorities, and the report of the reviewed sha.
    #[tokio::test]
    async fn the_pull_request_reviewer_tools_call_the_routes_of_the_sessions_request() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpSeat::PullRequestReviewer,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.get_diff(Parameters(GetDiffReq {
            since: Some("abc".into()),
        }))
        .await
        .expect("read the diff");
        mcp.submit_review(Parameters(SubmitReviewReq {
            event: ReviewEvent::RequestChanges,
            body: "One P0.".into(),
            comments: vec![ReviewFinding {
                path: "src/lib.rs".into(),
                line: 3,
                title: "An empty list panics".into(),
                body: "An empty list panics.".into(),
                priority: Priority::P0,
            }],
        }))
        .await
        .expect("post the review");
        mcp.report_pull_request(Parameters(ReportPullRequestReq {
            ready: None,
            head_sha: None,
            reviewed_sha: Some("abc".into()),
        }))
        .await
        .expect("report");
        mcp.resolve_thread(Parameters(ResolveThreadReq {
            comment_id: "01C".into(),
        }))
        .await
        .expect("resolve");

        let seen = seen.lock().expect("lock").clone();
        let calls: Vec<(String, String)> = seen
            .iter()
            .map(|s| (s.method.clone(), s.path.clone()))
            .collect();
        assert_eq!(
            calls,
            [
                ("GET".into(), "/v1/pull-requests/01PR/diff?since=abc".into()),
                ("POST".into(), "/v1/pull-requests/01PR/reviews".into()),
                ("POST".into(), "/v1/pull-requests/01PR/report".into()),
                (
                    "POST".into(),
                    "/v1/pull-requests/01PR/comments/01C/resolve".into()
                ),
            ]
        );
        let review: serde_json::Value = serde_json::from_str(&seen[1].body).expect("json");
        assert_eq!(
            review,
            serde_json::json!({"event": "request_changes", "body": "One P0.", "comments": [
                {"path": "src/lib.rs", "line": 3, "title": "An empty list panics",
                 "body": "An empty list panics.", "priority": "P0"}
            ]})
        );
        let report: serde_json::Value = serde_json::from_str(&seen[2].body).expect("json");
        assert_eq!(report["reviewed_sha"], "abc");
    }

    /// The orchestrator is never offered a model it cannot staff an agent on.
    ///
    /// A disabled entry is dropped rather than shown as off: the catalog is
    /// what a plan is sized from, and an entry an orchestrator is told about
    /// is one it pins sooner or later — which the daemon then refuses, in the
    /// middle of a plan, over a choice nobody could have made differently.
    #[test]
    fn the_catalog_an_agent_sees_holds_only_the_models_it_can_be_staffed_on() {
        let catalog = vec![
            serde_json::json!({"id": "claude-agent-acp:a", "agent_id": "claude-agent-acp", "enabled": true}),
            serde_json::json!({"id": "claude-agent-acp:b", "agent_id": "claude-agent-acp", "enabled": false}),
            serde_json::json!({"id": "codex-acp:c", "agent_id": "codex-acp", "enabled": true}),
        ];
        let ids = |models: Vec<serde_json::Value>| -> Vec<String> {
            models
                .into_iter()
                .map(|m| m["id"].as_str().unwrap().to_string())
                .collect()
        };

        assert_eq!(
            ids(of_agent(catalog.clone(), None)),
            ["claude-agent-acp:a", "codex-acp:c"]
        );
        assert_eq!(
            ids(of_agent(catalog.clone(), Some("claude-agent-acp".into()))),
            ["claude-agent-acp:a"],
            "and narrowing to an agent does not bring back what is off"
        );

        // An entry from a daemon that says nothing about it is offered: an
        // older daemon serves no `enabled` at all, and a catalog that went
        // empty against one would leave nothing to staff.
        let older =
            vec![serde_json::json!({"id": "codex-acp:gpt-5.6-sol", "agent_id": "codex-acp"})];
        assert_eq!(ids(of_agent(older, None)), ["codex-acp:gpt-5.6-sol"]);
    }

    /// The schema of one tool, as the agent reading the listing gets it.
    fn tool_schema(name: &str) -> serde_json::Value {
        let tool = AriadneMcp::tool_router()
            .list_all()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no {name} tool"));
        serde_json::to_value(&tool.input_schema).expect("schema")
    }

    /// A description is read on every tool listing, of every session: it says
    /// what the tool is for and what the agent has to decide, and stops there.
    /// The long ones were a second copy of a field's own doc.
    #[test]
    fn no_tool_is_described_at_length() {
        const CAP: usize = 300;
        for tool in AriadneMcp::tool_router().list_all() {
            let described = tool.description.as_deref().unwrap_or_default();
            assert!(
                described.len() <= CAP,
                "{} is described in {} characters, over the {CAP}",
                tool.name,
                described.len()
            );
        }
    }

    #[test]
    fn send_message_tells_agents_to_end_the_turn_after_a_question() {
        let tool = AriadneMcp::tool_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "send_message")
            .expect("the send_message tool");
        let description = tool.description.expect("the description");
        for rule in [
            "After a question, end your turn.",
            "Do not poll `read_messages`.",
            "Ariadne delivers the answer as a new turn.",
        ] {
            assert!(description.contains(rule), "the description and \"{rule}\"");
        }
    }

    /// An orchestrator server against a daemon that records what it is sent.
    fn orchestrator_at(endpoint: &str) -> AriadneMcp {
        server_at(
            McpSeat::Orchestrator,
            Client::resolve(Some(endpoint), None).with_session("01SESSION"),
        )
    }

    /// The skill catalog staffs task agents, so the orchestrator's own
    /// playbook stays out of this list even though the API lists it for edits.
    ///
    /// And an entry is a name and the one line about the skill, and nothing
    /// else. The document is what the agent staffed on the skill reads; an
    /// orchestrator choosing between skills reads the line, so a catalog that
    /// carried every `SKILL.md` spent tens of thousands of characters on what
    /// nobody chooses between.
    #[tokio::test]
    async fn the_skill_catalog_excludes_orchestrator_only_skills() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"[
                {"name":"orchestration","seat":"orchestrator","summary":"Plan a goal.","document":"","document_is_default":true,"builtin":true,"created_at":"","updated_at":""},
                {"name":"coding","seat":"task","summary":"Write code.","document":"Coding\n\nThe whole document of the skill.","document_is_default":true,"builtin":true,"created_at":"","updated_at":""}
            ]"#,
        )
        .await;

        let answered = orchestrator_at(&endpoint)
            .list_skills(Parameters(Empty {}))
            .await
            .expect("list skills");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "GET");
        assert_eq!(seen[0].path, "/v1/skills");
        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the skill catalog came back as something other than text");
        };
        let skills: Vec<serde_json::Value> =
            serde_json::from_str(&text.text).expect("the skill catalog is json");
        assert_eq!(skills.len(), 1, "{skills:?}");
        assert_eq!(
            skills[0],
            serde_json::json!({"name": "coding", "summary": "Write code."}),
            "the catalog is a name and a summary per skill, and nothing else"
        );
    }

    /// Opening a request posts the title and the body to the task's own
    /// pull-request endpoint, which is where the daemon runs the forge CLI:
    /// the tool carries only what the agent cannot read off the task or the
    /// repository itself.
    #[tokio::test]
    async fn opening_a_pull_request_posts_the_title_and_the_body() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.open_pull_request(Parameters(OpenPullRequestReq {
            title: "feat(cli): add the repo inspect command".into(),
            body: "## Summary\n- adds `ariadne repo inspect`".into(),
            draft: false,
        }))
        .await
        .expect("open the pull request");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/v1/tasks/01TASK/pull-request");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(
            sent,
            serde_json::json!({
                "title": "feat(cli): add the repo inspect command",
                "body": "## Summary\n- adds `ariadne repo inspect`",
                "draft": false,
            })
        );
    }

    /// Giving a task up moves it to `failed` with the reason on it, which is
    /// all the user is ever told; a reason with nothing in it is refused here
    /// rather than recorded, since a failed task saying nothing says nothing.
    #[tokio::test]
    async fn giving_a_task_up_records_the_reason_on_it() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.fail_task(Parameters(FailTaskReq {
            reason: "the crate it names was deleted upstream".into(),
        }))
        .await
        .expect("fail the task");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/v1/tasks/01TASK/transitions");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(sent["to"], serde_json::json!("failed"));
        assert_eq!(
            sent["reason"],
            serde_json::json!("the crate it names was deleted upstream")
        );

        for empty in ["", "  \n "] {
            let (endpoint, seen) = recording_daemon().await;
            let mcp = server_at(
                McpSeat::Agent,
                Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
            );
            let err = mcp
                .fail_task(Parameters(FailTaskReq {
                    reason: empty.into(),
                }))
                .await
                .expect_err("a failure with no reason");
            assert!(err.message.contains("needs a reason"), "{}", err.message);
            assert!(seen.lock().expect("lock").is_empty());
        }
    }

    /// Reading a task also reads its goal so the copied workflow columns sit
    /// beside the task and its agents.
    #[tokio::test]
    async fn reading_a_task_also_reads_its_goal_columns() {
        let (endpoint, seen) = recording_daemon_answering(r#"{"goal_id":"01GOAL"}"#).await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.get_task(Parameters(TaskIdOpt { task_id: None }))
            .await
            .expect("read the task");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0].method, "GET");
        assert_eq!(seen[0].path, "/v1/tasks/01TASK");
        assert_eq!(seen[1].method, "GET");
        assert_eq!(seen[1].path, "/v1/goals/01GOAL");
    }

    /// One verb, and it names the column it is for: the message goes to the
    /// agent that staffs that column, and carries no kind.
    ///
    /// There is nothing to ask with and nothing to answer with: a message is
    /// one agent telling another what it needs from it, and each one arrives
    /// at its agent as a turn — a channel that invites one back spends two turns
    /// saying nothing.
    #[tokio::test]
    async fn a_message_names_the_column_it_is_for() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"{"agents":[{"id":"01DEVELOP","step":"develop","skills":["coding"]},
                          {"id":"01REVIEW","step":"review","skills":["code-review"]}]}"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );

        mcp.send_message(Parameters(SendMessageReq {
            to: "develop".into(),
            body: "The retry is bounded by the caller, so the inner one is not.".into(),
            task_id: None,
        }))
        .await
        .expect("send_message");
        let sent = seen.lock().expect("lock").last().expect("sent").clone();
        assert_eq!(sent.method, "POST");
        assert_eq!(sent.path, "/v1/tasks/01TASK/messages");
        let sent: serde_json::Value = serde_json::from_str(&sent.body).expect("json");
        assert_eq!(sent.get("kind"), None, "a message has no kind");
        assert_eq!(sent["to_actor"], serde_json::json!("agent"));
        assert_eq!(sent["to_agent_id"], serde_json::json!("01DEVELOP"));
    }

    /// The orchestrator is addressed by what it is: a goal has one, and it is
    /// staffed on no task, so there is no id to name it by.
    #[tokio::test]
    async fn the_orchestrator_is_addressed_by_name_and_needs_no_agent_id() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"{"agents":[{"id":"01DEVELOP","step":"develop","skills":["coding"]}]}"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );

        mcp.send_message(Parameters(SendMessageReq {
            to: "orchestrator".into(),
            body: "The task names no CLI, and the spec it cites has one.".into(),
            task_id: None,
        }))
        .await
        .expect("send_message");

        let sent: serde_json::Value =
            serde_json::from_str(&seen.lock().expect("lock").last().expect("sent").body)
                .expect("json");
        assert_eq!(sent["to_actor"], serde_json::json!("orchestrator"));
        assert_eq!(sent["to_agent_id"], serde_json::Value::Null);
    }

    /// A `to` that names nobody is refused here, with the columns that would
    /// have worked: the agent reading the refusal is the one that has to fix
    /// it.
    #[tokio::test]
    async fn a_message_to_nobody_is_refused_with_the_addresses_that_would_work() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"{"agents":[{"id":"01DEVELOP","step":"develop","skills":["coding"]}]}"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );

        let err = mcp
            .send_message(Parameters(SendMessageReq {
                to: "01NOBODY".into(),
                body: "the flag moved".into(),
                task_id: None,
            }))
            .await
            .expect_err("no such agent");
        assert!(
            err.message.contains("01DEVELOP (develop)"),
            "{}",
            err.message
        );
        assert!(err.message.contains("orchestrator"), "{}", err.message);
        // The task was read to find that out, and nothing was written.
        assert!(
            seen.lock()
                .expect("lock")
                .iter()
                .all(|call| call.method == "GET"),
            "a message nobody could receive was sent anyway"
        );
    }

    #[tokio::test]
    async fn a_message_refusal_names_each_workflow_agent_with_its_column() {
        let (endpoint, _seen) = recording_daemon_answering(
            r#"{"agents":[
                {"id":"01DEVELOP","step":"develop","skills":["coding"]},
                {"id":"01REVIEW","step":"review","skills":["code-review"]}
            ]}"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let err = mcp
            .send_message(Parameters(SendMessageReq {
                to: "01NOBODY".into(),
                body: "Need help.".into(),
                task_id: None,
            }))
            .await
            .expect_err("no such agent");
        assert!(
            err.message.contains("01DEVELOP (develop)"),
            "{}",
            err.message
        );
        assert!(err.message.contains("01REVIEW (review)"), "{}", err.message);
    }

    /// What an orchestrator may write per agent is the schema an agent reads,
    /// and it is one staffing object per column: the column, what the agent
    /// knows, and what it runs on. An agent that still sent the old author
    /// and reviewer fields, or a landing, would have staffed nothing at all,
    /// so those must be gone rather than merely ignored.
    #[test]
    fn the_task_tools_take_agents_alone() {
        for tool in ["create_task", "update_task"] {
            let schema = tool_schema(tool);
            let props = schema["properties"].as_object().expect("properties");
            assert!(props.contains_key("agents"), "{tool} takes no agents");
            for gone in [
                "authors",
                "author",
                "reviewers",
                "author_model",
                "author_effort",
                "model",
                "effort",
                "landing",
            ] {
                assert!(!props.contains_key(gone), "{tool} still takes {gone}");
            }
            let agent = schema["$defs"]["AgentReq"]["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{tool} has no agent object"));
            for field in ["step", "skills", "model", "effort", "brief"] {
                assert!(agent.contains_key(field), "{tool}: no agent {field}");
            }
            assert!(
                schema
                    .get("$defs")
                    .is_none_or(|defs| defs.get("LandingReq").is_none()),
                "{tool} still defines a landing"
            );
        }
    }

    /// A pin the orchestrator named is the pin the daemon is asked for, column
    /// by column: whatever this passes on is what the task is cut at, and a
    /// field quietly left out here is a task running on something nobody
    /// chose.
    #[tokio::test]
    async fn a_created_task_is_pinned_to_what_the_orchestrator_named() {
        let (endpoint, seen) = recording_daemon().await;
        orchestrator_at(&endpoint)
            .create_task(Parameters(CreateTaskReq {
                title: "Pin the effort".into(),
                description: "Beside the model.".into(),
                agents: vec![
                    AgentReq {
                        step: "develop".into(),
                        model: "codex-acp:gpt-5.6-sol".into(),
                        effort: Some("xhigh".into()),
                        skills: Some(vec!["coding".into()]),
                        brief: Some("Keep the public API.".into()),
                    },
                    AgentReq {
                        step: "review".into(),
                        model: "claude-agent-acp:claude-haiku-4-5".into(),
                        effort: Some("low".into()),
                        skills: Some(vec!["code-review".into()]),
                        brief: None,
                    },
                ],
                depends_on: None,
                repo_id: None,
            }))
            .await
            .expect("create the task");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/v1/goals/01GOAL/tasks");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(
            sent["agents"],
            serde_json::json!([
                {
                    "step": "develop",
                    "skills": ["coding"],
                    "model": "codex-acp:gpt-5.6-sol",
                    "effort": "xhigh",
                    "brief": "Keep the public API.",
                },
                {
                    "step": "review",
                    "skills": ["code-review"],
                    "model": "claude-agent-acp:claude-haiku-4-5",
                    "effort": "low",
                    "brief": null,
                },
            ])
        );
    }

    /// One staffing object per column: skills left out travel as an empty
    /// list, which tells the daemon to use the column's own, and an empty
    /// staffing is refused before anything is sent.
    #[tokio::test]
    async fn a_workflow_task_staffs_one_agent_for_each_column() {
        let (endpoint, seen) = recording_daemon().await;
        orchestrator_at(&endpoint)
            .create_task(Parameters(CreateTaskReq {
                title: "Work it".into(),
                description: String::new(),
                agents: vec![
                    AgentReq {
                        step: "develop".into(),
                        model: "codex-acp:gpt-5.6-sol".into(),
                        effort: None,
                        skills: None,
                        brief: None,
                    },
                    AgentReq {
                        step: "review".into(),
                        model: "codex-acp:gpt-5.6-luna".into(),
                        effort: Some("high".into()),
                        skills: Some(vec!["code-review".into()]),
                        brief: None,
                    },
                ],
                depends_on: None,
                repo_id: None,
            }))
            .await
            .expect("create the task");
        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        let body: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(
            body["agents"][0].get("seat"),
            None,
            "a staffing names no seat"
        );
        assert_eq!(body["agents"][0]["step"], "develop");
        assert_eq!(body["agents"][0]["skills"], serde_json::json!([]));
        assert_eq!(body["agents"][1]["step"], "review");
        assert_eq!(
            body["agents"][1]["skills"],
            serde_json::json!(["code-review"])
        );

        let (endpoint, seen) = recording_daemon().await;
        let err = orchestrator_at(&endpoint)
            .create_task(Parameters(CreateTaskReq {
                title: "Work it".into(),
                description: String::new(),
                agents: vec![],
                depends_on: None,
                repo_id: None,
            }))
            .await
            .expect_err("an empty staffing is refused");
        assert!(
            err.message.contains("one for each column"),
            "{}",
            err.message
        );
        assert!(seen.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn step_tools_post_their_bodies_to_the_step_routes() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpSeat::Agent,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.complete_step(Parameters(CompleteStepReq {
            reason: "The code is ready.".into(),
            merge_commit: Some("abc".into()),
        }))
        .await
        .expect("complete");
        mcp.fail_step(Parameters(FailStepReq {
            reason: "Fix the test.".into(),
        }))
        .await
        .expect("fail");
        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen[0].path, "/v1/tasks/01TASK/step/complete");
        assert_eq!(seen[1].path, "/v1/tasks/01TASK/step/fail");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&seen[0].body).expect("json"),
            serde_json::json!({"reason": "The code is ready.", "merge_commit": "abc"})
        );
    }

    /// The word that clears an *effort* travels as it was written — the
    /// daemon is what knows "default" runs the model at the CLI's own — while
    /// the same word as a model is refused here, where the agent that typed
    /// it reads the answer: a model is required, and there is nothing to
    /// clear one to.
    #[tokio::test]
    async fn an_edit_takes_default_for_the_effort_and_refuses_it_as_a_model() {
        let (endpoint, seen) = recording_daemon().await;
        orchestrator_at(&endpoint)
            .update_task(Parameters(UpdateTaskReq {
                task_id: "01TASK".into(),
                title: None,
                description: None,
                agents: Some(vec![AgentReq {
                    step: "review".into(),
                    skills: Some(vec!["code-review".into()]),
                    model: "codex-acp:gpt-5.6-luna".into(),
                    effort: Some("default".into()),
                    brief: None,
                }]),
                depends_on: None,
            }))
            .await
            .expect("edit the task");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "PATCH");
        assert_eq!(seen[0].path, "/v1/tasks/01TASK");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(sent["title"], serde_json::Value::Null, "left alone");
        assert_eq!(
            sent["agents"],
            serde_json::json!([{
                "step": "review",
                "skills": ["code-review"],
                "model": "codex-acp:gpt-5.6-luna",
                "effort": "default",
                "brief": null,
            }])
        );

        let (endpoint, seen) = recording_daemon().await;
        let err = orchestrator_at(&endpoint)
            .update_task(Parameters(UpdateTaskReq {
                task_id: "01TASK".into(),
                title: None,
                description: None,
                agents: Some(vec![AgentReq {
                    step: "develop".into(),
                    skills: None,
                    model: "default".into(),
                    effort: None,
                    brief: None,
                }]),
                depends_on: None,
            }))
            .await
            .expect_err("default is no model");
        assert!(
            err.message.contains("a model is required"),
            "{}",
            err.message
        );
        assert!(err.message.contains("develop"), "{}", err.message);
        assert!(
            seen.lock().expect("lock").is_empty(),
            "nothing was sent for the daemon to refuse"
        );
    }

    /// `switch_session` moves a stuck or exhausted agent off its session, so
    /// it is the orchestrator's alone: a column's agent works its own task,
    /// with no session of another agent's to read off `get_task`.
    #[test]
    fn switch_session_is_offered_to_the_orchestrator_alone() {
        for (seat, offered) in [
            (McpSeat::Orchestrator, true),
            (McpSeat::Agent, false),
            (McpSeat::PullRequestReviewer, false),
        ] {
            let mcp = server_at(
                seat.clone(),
                Client::resolve(Some("http://127.0.0.1:1"), None),
            );
            assert_eq!(mcp.allows("switch_session"), offered, "{seat:?}");
        }
    }

    /// The pin named reaches the switch endpoint whole, keyed on the session
    /// id rather than on the task: a session is what runs an agent, and the
    /// one `get_task` names beside it is the one this call moves.
    #[tokio::test]
    async fn switch_session_posts_the_pin_to_the_switch_endpoint() {
        let (endpoint, seen) = recording_daemon().await;
        orchestrator_at(&endpoint)
            .switch_session(Parameters(SwitchSessionReq {
                session_id: "01SESSION2".into(),
                model: "codex-acp:gpt-5.6-sol".into(),
                effort: Some("high".into()),
            }))
            .await
            .expect("switch the session");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/v1/sessions/01SESSION2/switch");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(
            sent,
            serde_json::json!({"model": "codex-acp:gpt-5.6-sol", "effort": "high"})
        );
    }

    /// A model is required to run a session on, so `default` — the word that
    /// clears an *effort* — is refused here, the way `update_task` refuses it
    /// as a model.
    #[tokio::test]
    async fn switch_session_refuses_default_as_a_model() {
        let (endpoint, seen) = recording_daemon().await;
        let err = orchestrator_at(&endpoint)
            .switch_session(Parameters(SwitchSessionReq {
                session_id: "01SESSION2".into(),
                model: "default".into(),
                effort: None,
            }))
            .await
            .expect_err("default is no model");
        assert!(
            err.message.contains("a model is required"),
            "{}",
            err.message
        );
        assert!(
            seen.lock().expect("lock").is_empty(),
            "nothing was sent for the daemon to refuse"
        );
    }

    /// The catalog is what an orchestrator sizes a task from, so it reaches
    /// it whole — what each model is for, and every effort it takes with what
    /// that effort buys — and an agent narrows the answer rather than
    /// the question, since `GET /v1/models` takes no filter.
    #[tokio::test]
    async fn the_catalog_reaches_the_orchestrator_with_the_efforts_on_it() {
        const CATALOG: &str = r#"[
            {"id": "codex-acp:gpt-5.6-sol", "agent_id": "codex-acp",
             "description": "frontier", "rank": "frontier",
             "efforts": [
               {"id": "low", "description": "lighter reasoning", "default": false},
               {"id": "high", "description": "greater depth", "default": true},
               {"id": "xhigh", "description": "deeper still", "default": false}
             ]},
            {"id": "claude-agent-acp:claude-haiku-4-5", "agent_id": "claude-agent-acp",
             "description": "fast", "efforts": []}
        ]"#;
        for (filter, ids) in [
            (
                None,
                vec!["codex-acp:gpt-5.6-sol", "claude-agent-acp:claude-haiku-4-5"],
            ),
            (Some("codex-acp"), vec!["codex-acp:gpt-5.6-sol"]),
            (Some("opencode-acp"), vec![]),
        ] {
            let (endpoint, seen) = recording_daemon_answering(CATALOG).await;
            let answered = orchestrator_at(&endpoint)
                .list_models(Parameters(ListModelsReq {
                    agent_id: filter.map(str::to_string),
                }))
                .await
                .expect("list the models");

            let seen = seen.lock().expect("lock").clone();
            assert_eq!(seen.len(), 1, "{seen:?}");
            assert_eq!(seen[0].method, "GET");
            assert_eq!(seen[0].path, "/v1/models");

            let ContentBlock::Text(text) = &answered.content[0] else {
                panic!("the catalog came back as something other than text");
            };
            let models: Vec<serde_json::Value> =
                serde_json::from_str(&text.text).expect("the catalog is json");
            assert_eq!(
                models.iter().map(|m| m["id"].clone()).collect::<Vec<_>>(),
                ids.iter()
                    .map(|id| serde_json::json!(id))
                    .collect::<Vec<_>>(),
                "filtered by {filter:?}"
            );
            if filter.is_none() {
                assert_eq!(models[0]["description"], serde_json::json!("frontier"));
                assert_eq!(
                    models[0]["efforts"]
                        .as_array()
                        .expect("the efforts")
                        .iter()
                        .map(|e| e["id"].clone())
                        .collect::<Vec<_>>(),
                    ["low", "high", "xhigh"].map(|id| serde_json::json!(id))
                );
                assert_eq!(models[0]["efforts"][1]["default"], serde_json::json!(true));
                assert_eq!(
                    models[0]["efforts"][1]["description"],
                    serde_json::json!("greater depth")
                );
                assert_eq!(models[1]["efforts"], serde_json::json!([]));
            }
        }
    }

    /// The rank the user gave a model reaches the orchestrator, and the tool
    /// says what a rank is.
    ///
    /// The rank is the only thing in the catalog about cost, so a staffing
    /// rule that reads it needs both halves: the value on every entry, and a
    /// description that names the four ranks and the direction of the ladder.
    /// An unranked entry comes through as `null`, which is the orchestrator's
    /// cue to size that model from its description instead.
    #[tokio::test]
    async fn the_rank_of_each_model_reaches_the_orchestrator() {
        const CATALOG: &str = r#"[
            {"id": "codex-acp:gpt-5.6-sol", "agent_id": "codex-acp",
             "rank": "frontier", "efforts": []},
            {"id": "codex-acp:gpt-5.6-mini", "agent_id": "codex-acp",
             "rank": "fast", "efforts": []},
            {"id": "opencode-acp:ollama/llama3", "agent_id": "opencode-acp",
             "rank": "local", "efforts": []},
            {"id": "claude-agent-acp:claude-haiku-4-5",
             "agent_id": "claude-agent-acp", "efforts": []}
        ]"#;
        let (endpoint, _) = recording_daemon_answering(CATALOG).await;
        let answered = orchestrator_at(&endpoint)
            .list_models(Parameters(ListModelsReq { agent_id: None }))
            .await
            .expect("list the models");

        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the catalog came back as something other than text");
        };
        let models: Vec<serde_json::Value> =
            serde_json::from_str(&text.text).expect("the catalog is json");
        assert_eq!(
            models.iter().map(|m| m["rank"].clone()).collect::<Vec<_>>(),
            [
                serde_json::json!("frontier"),
                serde_json::json!("fast"),
                serde_json::json!("local"),
                serde_json::Value::Null,
            ]
        );

        let tool = AriadneMcp::tool_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "list_models")
            .expect("the tool");
        let description = tool.description.expect("the description").to_string();
        for part in [
            "`rank`, the standing the user set",
            "`frontier`, `balanced`, `fast`, `local` or `null`",
            "a ladder from `fast` up",
        ] {
            assert!(
                description.contains(part),
                "the description says nothing of \"{part}\": {description}"
            );
        }
    }

    /// A default read of the channel is a delivery: the daemon narrows it to
    /// this session's own agent and stamps what it hands over, so a message
    /// the agent has already had as a turn is not sent to it a second time as
    /// the whole thread. `all` is the whole thread.
    #[tokio::test]
    async fn a_default_read_takes_delivery_and_all_reads_the_whole_thread() {
        for (all, path) in [
            (None, "/v1/tasks/01TASK/messages?deliver=true"),
            (Some(false), "/v1/tasks/01TASK/messages?deliver=true"),
            (Some(true), "/v1/tasks/01TASK/messages"),
        ] {
            let (endpoint, seen) = recording_daemon_answering("[]").await;
            server_at(
                McpSeat::Agent,
                Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
            )
            .read_messages(Parameters(ReadMessagesReq { task_id: None, all }))
            .await
            .expect("read the messages");

            let seen = seen.lock().expect("lock").clone();
            assert_eq!(seen.len(), 1, "{seen:?}");
            assert_eq!(seen[0].method, "GET");
            assert_eq!(seen[0].path, path, "all = {all:?}");
        }
    }

    /// The orchestrator reads the channel of its goal, which is its inbox: it
    /// is staffed on no task, so a read that asked for one would refuse the
    /// one seat whose messages are all on the goal.
    #[tokio::test]
    async fn the_orchestrator_reads_the_channel_of_its_goal() {
        let (endpoint, seen) = recording_daemon_answering("[]").await;
        let mut mcp = orchestrator_at(&endpoint);
        mcp.task_id = None;
        mcp.read_messages(Parameters(ReadMessagesReq {
            task_id: None,
            all: None,
        }))
        .await
        .expect("read the messages");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].path, "/v1/goals/01GOAL/messages?deliver=true");
    }

    /// A column id is an address: a task staffs one agent per column, so the
    /// word names one reader. An id still addresses too, and a `to` that is
    /// neither is refused with every column and the id that staffs it, so
    /// the sender picks a reader rather than guessing again.
    #[test]
    fn a_column_id_addresses_the_agent_that_staffs_it() {
        let agents = vec![
            serde_json::json!({"id": "01DEVELOP", "step": "develop"}),
            serde_json::json!({"id": "01REVIEW", "step": "review"}),
        ];
        assert_eq!(
            addressee("develop", &agents).expect("the develop column"),
            (Actor::Agent, Some("01DEVELOP".to_string()))
        );
        assert_eq!(
            addressee("Review", &agents).expect("the review column, however spelled"),
            (Actor::Agent, Some("01REVIEW".to_string()))
        );
        assert_eq!(
            addressee("01REVIEW", &agents).expect("by id"),
            (Actor::Agent, Some("01REVIEW".to_string()))
        );
        assert_eq!(
            addressee("orchestrator", &agents).expect("the orchestrator"),
            (Actor::Orchestrator, None)
        );

        for old in ["author", "reviewer", "01NOBODY"] {
            let err = addressee(old, &agents).expect_err("no such agent");
            assert!(
                err.message.contains("01DEVELOP (develop)"),
                "{}",
                err.message
            );
            assert!(err.message.contains("01REVIEW (review)"), "{}", err.message);
            assert!(err.message.contains("orchestrator"), "{}", err.message);
        }
    }

    /// The body of a message is taken under the name agents write it with.
    ///
    /// `message` is the commonest of those, and every one of them used to be
    /// refused as a missing `body` — a whole turn spent to send the same
    /// words under another key.
    #[test]
    fn a_message_body_is_taken_as_message_too() {
        let req: SendMessageReq = serde_json::from_value(serde_json::json!({
            "to": "develop",
            "message": "The bound is the caller's.",
        }))
        .expect("a body written as `message`");
        assert_eq!(req.body, "The bound is the caller's.");
    }
}
