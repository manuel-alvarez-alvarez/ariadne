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
use ariadne_api::skills::{SkillDto, SkillSeat};
use ariadne_api::tasks::{
    AgentAssignment, CreateTaskRequest, RecordPullRequestRequest, TransitionRequest,
    UpdateTaskRequest,
};
use ariadne_core::{Actor, Landing, MessageKind, Seat, TaskStatus};

use super::{AriadneMcp, json_result, to_mcp_err};

// ---------- tool parameter types ----------

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct Empty {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct TaskIdOpt {
    /// Task id. Omit it for your own task.
    pub task_id: Option<String>,
}

/// One agent an orchestrator staffs on a task: the skills it loads, and the
/// model and effort this task is worth.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct AgentReq {
    /// The names of the skills this agent loads, from `list_skills`. They are
    /// the whole of what it can do.
    pub skills: Vec<String>,
    /// What it runs on, `<agent_kind>:<model>` as `list_models` spells it.
    /// Required: every agent names its CLI and its model.
    pub model: String,
    /// An `efforts[].id` `list_models` lists for that model. Omit it for the
    /// default effort.
    pub effort: Option<String>,
    /// What to tell this agent beyond the task. Omit it where the task says
    /// everything.
    pub brief: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateTaskReq {
    pub title: String,
    pub description: String,
    /// The one agent that writes the task. It owns the task to the end.
    pub author: AgentReq,
    /// The agents that review the task, in review order. Staff at least one
    /// wherever the work can be judged. Leave it empty only where there is
    /// nothing to review, such as a release: the task is then approved as
    /// soon as its author asks.
    pub reviewers: Vec<AgentReq>,
    /// Ids of the tasks that must merge before this one starts.
    pub depends_on: Option<Vec<String>>,
    /// Repository id. Pass it only where the goal works in several.
    pub repo_id: Option<String>,
    /// How the task ends, as the user agreed it: `merge` puts the change on
    /// the base branch, `pull_request` opens a request and sees it through,
    /// `none` lands nothing. Omit it for the way the repository takes a
    /// change.
    pub landing: Option<LandingReq>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct UpdateTaskReq {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// What the author runs on, `<agent_kind>:<model>`. Omit it to keep the
    /// model it has; a model is required, so `default` is refused.
    pub author_model: Option<String>,
    /// An `efforts[].id` for that model. `default` puts it back on the
    /// default effort.
    pub author_effort: Option<String>,
    /// The reviewers, in review order. This list replaces the whole list, and
    /// an empty list takes every reviewer off the task.
    pub reviewers: Option<Vec<AgentReq>>,
    /// The ids of the tasks that must merge first. This list replaces the
    /// whole list.
    pub depends_on: Option<Vec<String>>,
    /// How the task ends: `merge`, `pull_request` or `none`.
    pub landing: Option<LandingReq>,
}

/// The one task a supervising tool acts on, by id.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct TaskId {
    /// Task id, as `list_tasks` gives it.
    pub task_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListModelsReq {
    /// Filter: claude_code | codex | opencode
    pub agent_kind: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct RequestReviewReq {
    /// Your summary of the change, for the reviewers.
    pub summary: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct FailTaskReq {
    /// Why you cannot do the task as written. Ariadne records it on the
    /// task, and the user reads only this.
    pub reason: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct FinishTaskReq {
    /// The sha of the merge commit on the base branch. Omit it only where
    /// the task lands nothing.
    pub merge_commit: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct RecordPullRequestReq {
    /// The URL of the pull request, as `gh pr create` or `glab mr create`
    /// printed it.
    pub url: String,
}

/// The three ways a task can end, as the two task tools take them. A local
/// spelling of [`Landing`], because the schema an agent reads is derived from
/// the parameter types here.
#[derive(Clone, Copy, Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum LandingReq {
    /// The author puts the change on the base branch itself.
    Merge,
    /// The author opens a request and sees it through to its merge.
    PullRequest,
    /// Nothing is landed: a published tag, a filed report, a document that
    /// lives elsewhere.
    None,
}

impl From<LandingReq> for Landing {
    fn from(req: LandingReq) -> Landing {
        match req {
            LandingReq::Merge => Landing::Merge,
            LandingReq::PullRequest => Landing::PullRequest,
            LandingReq::None => Landing::None,
        }
    }
}

/// The two verdicts a review round ends in, as the one verdict tool takes
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
    /// A note on an approval. On a change request, the feedback the author
    /// starts again on, and required there.
    pub body: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SendMessageReq {
    /// Who to write to: the id of an agent `get_task` lists, or
    /// `orchestrator`.
    pub to: String,
    /// What it needs from you, whole. Nobody answers it.
    pub body: String,
    /// The task it is about. Omit it for your own task.
    pub task_id: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ReadMessagesReq {
    /// The task whose channel to read. Omit it for your own task.
    pub task_id: Option<String>,
}

// ---------- helpers ----------

/// One agent an orchestrator staffed, as the API takes it: where it sits,
/// what it knows, and the pin it runs at.
fn assignment(seat: Seat, agent: AgentReq) -> AgentAssignment {
    AgentAssignment {
        seat,
        skills: agent.skills,
        model: agent.model,
        effort: agent.effort,
        brief: agent.brief,
    }
}

/// The catalog narrowed to one agent CLI, or all of it, and always to the
/// models an agent can actually be staffed on. Entries pass through as the
/// daemon wrote them: what a model is called and what it can be run at is the
/// daemon's answer, not this file's.
///
/// A model the user turned off is dropped rather than shown as off. The
/// catalog is what an orchestrator sizes from, and an entry it is told about
/// is one it will pin sooner or later — which the daemon then refuses, in the
/// middle of a plan, over a choice nobody could have made differently.
fn of_agent(models: Vec<serde_json::Value>, agent_kind: Option<String>) -> Vec<serde_json::Value> {
    models
        .into_iter()
        .filter(|m| m["enabled"] != serde_json::Value::Bool(false))
        .filter(|m| match &agent_kind {
            Some(kind) => m["agent_kind"] == serde_json::Value::String(kind.clone()),
            None => true,
        })
        .collect()
}

/// The verdict the daemon records, refusing a change request with nothing in
/// it: the body is what the author is resumed with, so a round that asks for
/// changes and says nothing asks for nothing.
///
/// A verdict is a message to the author like any other. What makes it close a
/// round is its kind.
fn verdict_message(verdict: Verdict, body: Option<String>) -> Result<SendMessageRequest, McpError> {
    let body = body.map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
    let (kind, body) = match verdict {
        Verdict::Approve => (
            MessageKind::Approve,
            body.unwrap_or_else(|| "Approved.".to_string()),
        ),
        Verdict::RequestChanges => {
            let Some(body) = body else {
                return Err(McpError::invalid_params(
                    "request_changes needs a body: the feedback the author is resumed with",
                    None,
                ));
            };
            (MessageKind::RequestChanges, body)
        }
    };
    Ok(SendMessageRequest {
        kind,
        to_actor: Actor::Author,
        // Filled in by the caller, which knows the task's author.
        to_agent_id: None,
        body,
    })
}

/// Who a message is for, as an agent spells it: `orchestrator`, or the id of
/// an agent the task staffs.
///
/// The seat is looked up rather than asked for. An agent reading `get_task`
/// has the ids in front of it and no reason to also work out which seat each
/// one sits in — and a `to` that named the wrong seat would be refused for a
/// reason nobody could act on.
fn addressee(to: &str, agents: &[serde_json::Value]) -> Result<(Actor, Option<String>), McpError> {
    if to.eq_ignore_ascii_case("orchestrator") {
        return Ok((Actor::Orchestrator, None));
    }
    let Some(agent) = agents.iter().find(|a| a["id"] == to) else {
        let known: Vec<&str> = agents.iter().filter_map(|a| a["id"].as_str()).collect();
        return Err(McpError::invalid_params(
            format!(
                "no agent {to} on this task. Say `orchestrator`, or one of: {}",
                known.join(", ")
            ),
            None,
        ));
    };
    let actor = match agent["seat"].as_str() {
        Some("author") => Actor::Author,
        Some("reviewer") => Actor::Reviewer,
        _ => {
            return Err(McpError::invalid_params(
                format!("agent {to} sits nowhere"),
                None,
            ));
        }
    };
    Ok((actor, Some(to.to_string())))
}

#[tool_router(vis = "pub(super)")]
impl AriadneMcp {
    #[tool(
        description = "Read a task: the status, the branch, the dependencies, and the agents staffed on it with the skills each one loads."
    )]
    async fn get_task(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        json_result(self.get(&self.task_path(req.task_id, "")?).await?)
    }

    // ---- orchestrator ----

    #[tool(
        description = "Create one task in the goal. Staff one author, and the reviewers the user agreed it needs. Give each agent the skills its work needs (`list_skills`) and one model from `list_models` — every agent names its model. Say how it ends with `landing`."
    )]
    async fn create_task(
        &self,
        Parameters(req): Parameters<CreateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/goals/{}/tasks", self.goal_id);
        let body = CreateTaskRequest {
            title: req.title,
            description: req.description,
            repo_id: req.repo_id,
            agents: std::iter::once(assignment(Seat::Author, req.author))
                .chain(
                    req.reviewers
                        .into_iter()
                        .map(|r| assignment(Seat::Reviewer, r)),
                )
                .collect(),
            depends_on: req.depends_on.unwrap_or_default(),
            landing: req.landing.map(Into::into),
        };
        json_result(self.post(&path, &body).await?)
    }

    #[tool(
        description = "Edit a task that has not started: its title, description, reviewers, dependencies, ending, or the model and effort of its author. `reviewers` replaces the whole list. A model is required, so `default` is no model; an omitted `author_model` keeps the one the task has."
    )]
    async fn update_task(
        &self,
        Parameters(req): Parameters<UpdateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        // Refused here, where the agent that typed it reads the answer: the
        // word used to clear a model, and there is no longer anything to
        // clear one to.
        if req.author_model.as_deref() == Some("default") {
            return Err(McpError::invalid_params(
                "`default` is no model — a model is required, so name one from \
                 `list_models`, or omit `author_model` to keep the task's own",
                None,
            ));
        }
        let body = UpdateTaskRequest {
            title: req.title,
            description: req.description,
            model: req.author_model,
            effort: req.author_effort,
            reviewers: req.reviewers.map(|reviewers| {
                reviewers
                    .into_iter()
                    .map(|r| assignment(Seat::Reviewer, r))
                    .collect()
            }),
            depends_on: req.depends_on,
            landing: req.landing.map(Into::into),
        };
        let path = format!("/v1/tasks/{}", req.task_id);
        let value = self.client.patch_json(&path, &body).await;
        json_result(value.map_err(to_mcp_err)?)
    }

    #[tool(
        description = "List the agent CLIs and models a slot can run on. Each entry gives:\n- a description and a `tier`, frontier to fast, or `unknown` with no bands or shapes\n- `cost` and `speed` 1-5, low to high, slow to fast\n- `best_for` and `avoid_for` shapes\n- `efforts`, each an id and what it buys, one `default`"
    )]
    async fn list_models(
        &self,
        Parameters(req): Parameters<ListModelsReq>,
    ) -> Result<CallToolResult, McpError> {
        // The catalog is the union and takes no filter, so an agent kind
        // narrows what it answered rather than what was asked for.
        let models: Vec<serde_json::Value> = self.get("/v1/models").await?;
        json_result(serde_json::Value::Array(of_agent(models, req.agent_kind)))
    }

    #[tool(
        description = "List the skills an agent can load. Each entry gives the name and one line saying what that skill is for. Give an agent the skills its work needs and no more."
    )]
    async fn list_skills(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let skills: Vec<SkillDto> = self.get("/v1/skills").await?;
        json_result(
            serde_json::to_value(
                skills
                    .into_iter()
                    .filter(|skill| skill.seat == SkillSeat::Task)
                    .collect::<Vec<_>>(),
            )
            .expect("skills serialize"),
        )
    }

    #[tool(
        description = "Finalize the plan. This call starts every task of the plan and ends planning."
    )]
    async fn finalize_plan(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/goals/{}/finalize", self.goal_id);
        json_result(self.post(&path, &FinalizePlanRequest {}).await?)
    }

    #[tool(
        description = "List every task of the goal, with its status, how it ends, and the agents staffed on it. This is how you see where the goal stands."
    )]
    async fn list_tasks(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/tasks?goal_id={}", self.goal_id);
        json_result(self.get::<serde_json::Value>(&path).await?)
    }

    #[tool(
        description = "Start a failed task again, from the beginning. Rewrite it with `update_task` first where it failed on how it was written."
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
        description = "End the goal. Call it once every task is finished or cancelled and the goal is met. Ariadne refuses it while any task is still going."
    )]
    async fn complete_goal(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/goals/{}/complete", self.goal_id);
        json_result(self.post(&path, &CompleteGoalRequest {}).await?)
    }

    // ---- author ----

    #[tool(
        description = "Submit your task for review. The reviewers read your summary and nothing else: what changed, why, how you verified it."
    )]
    async fn request_review(
        &self,
        Parameters(req): Parameters<RequestReviewReq>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.transition(TaskStatus::UnderReview, Some(req.summary), None)
                .await?,
        )
    }

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
            self.transition(TaskStatus::Failed, Some(reason.to_string()), None)
                .await?,
        )
    }

    #[tool(
        description = "End the task. Report the sha your branch landed on its base branch as, or nothing at all where the task lands nothing."
    )]
    async fn finish_task(
        &self,
        Parameters(req): Parameters<FinishTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.transition(TaskStatus::Finished, None, req.merge_commit)
                .await?,
        )
    }

    #[tool(
        description = "Report the URL of the pull request or merge request you opened for this task."
    )]
    async fn record_pull_request(
        &self,
        Parameters(req): Parameters<RecordPullRequestReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.task_path(None, "/pull-request")?;
        json_result(
            self.post(&path, &RecordPullRequestRequest { url: req.url })
                .await?,
        )
    }

    // ---- reviewer ----

    #[tool(description = "Read the diff of the branch under review against its base branch.")]
    async fn get_diff(&self, Parameters(_): Parameters<Empty>) -> Result<CallToolResult, McpError> {
        // Plain-text endpoint: no JSON decoding.
        let diff = self
            .client
            .get_text(&self.task_path(None, "/diff")?)
            .await
            .map_err(to_mcp_err)?;
        Ok(CallToolResult::success(vec![ContentBlock::text(diff)]))
    }

    #[tool(
        description = "Give your verdict on the change. Approve it, or request changes. A change request carries the feedback the author starts again on. Where something blocks the review, request changes and name it."
    )]
    async fn submit_verdict(
        &self,
        Parameters(req): Parameters<SubmitVerdictReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.task_path(None, "/messages")?;
        let mut body = verdict_message(req.verdict, req.body)?;
        body.to_agent_id = Some(self.author_of(None).await?);
        json_result(self.post(&path, &body).await?)
    }

    // ---- everyone ----

    #[tool(
        description = "Send one message to another agent, `to` is an agent id from `get_task`, or `orchestrator`. Use it only to ask questions or to answer questions, no acknowledgements, no thanks, and nothing about what you are going to do next."
    )]
    async fn send_message(
        &self,
        Parameters(req): Parameters<SendMessageReq>,
    ) -> Result<CallToolResult, McpError> {
        self.write_message(req.task_id, MessageKind::Message, &req.to, req.body)
            .await
    }

    #[tool(
        description = "Read everything the agents of a task have said to each other, oldest first: the messages, the review requests and the verdicts."
    )]
    async fn read_messages(
        &self,
        Parameters(req): Parameters<ReadMessagesReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.task_path(req.task_id, "/messages")?;
        json_result(self.get::<serde_json::Value>(&path).await?)
    }
}

impl AriadneMcp {
    /// Write one message about `task_id` — the session's own where it names
    /// none — to whoever `to` spells.
    async fn write_message(
        &self,
        task_id: Option<String>,
        kind: MessageKind,
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
            kind,
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

    /// The id of a task's author, which is who a verdict is for.
    async fn author_of(&self, task_id: Option<String>) -> Result<String, McpError> {
        self.agents_of(task_id)
            .await?
            .into_iter()
            .find(|a| a["seat"] == "author")
            .and_then(|a| a["id"].as_str().map(str::to_string))
            .ok_or_else(|| McpError::internal_error("this task has no author", None))
    }

    /// Move this session's own task, which is the only one an author may
    /// move.
    async fn transition(
        &self,
        to: TaskStatus,
        reason: Option<String>,
        merge_commit: Option<String>,
    ) -> Result<serde_json::Value, McpError> {
        self.post(
            &self.task_path(None, "/transitions")?,
            &TransitionRequest {
                to,
                reason,
                merge_commit,
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
    use crate::commands::mcp::tests::{recording_daemon, recording_daemon_answering, server_at};

    /// The orchestrator is never offered a model it cannot staff an agent on.
    ///
    /// A disabled entry is dropped rather than shown as off: the catalog is
    /// what a plan is sized from, and an entry an orchestrator is told about
    /// is one it pins sooner or later — which the daemon then refuses, in the
    /// middle of a plan, over a choice nobody could have made differently.
    #[test]
    fn the_catalog_an_agent_sees_holds_only_the_models_it_can_be_staffed_on() {
        let catalog = vec![
            serde_json::json!({"id": "claude_code:a", "agent_kind": "claude_code", "enabled": true}),
            serde_json::json!({"id": "claude_code:b", "agent_kind": "claude_code", "enabled": false}),
            serde_json::json!({"id": "codex:c", "agent_kind": "codex", "enabled": true}),
        ];
        let ids = |models: Vec<serde_json::Value>| -> Vec<String> {
            models
                .into_iter()
                .map(|m| m["id"].as_str().unwrap().to_string())
                .collect()
        };

        assert_eq!(
            ids(of_agent(catalog.clone(), None)),
            ["claude_code:a", "codex:c"]
        );
        assert_eq!(
            ids(of_agent(catalog.clone(), Some("claude_code".into()))),
            ["claude_code:a"],
            "and narrowing to a CLI does not bring back what is off"
        );

        // An entry from a daemon that says nothing about it is offered: an
        // older daemon serves no `enabled` at all, and a catalog that went
        // empty against one would leave nothing to staff.
        let older = vec![serde_json::json!({"id": "codex:gpt-5.6-sol", "agent_kind": "codex"})];
        assert_eq!(ids(of_agent(older, None)), ["codex:gpt-5.6-sol"]);
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

    /// An orchestrator server against a daemon that records what it is sent.
    fn orchestrator_at(endpoint: &str) -> AriadneMcp {
        server_at(
            McpSeat::Orchestrator,
            Client::resolve(Some(endpoint), None).with_session("01SESSION"),
        )
    }

    /// The skill catalog staffs task agents, so the orchestrator's own
    /// playbook stays out of this list even though the API lists it for edits.
    #[tokio::test]
    async fn the_skill_catalog_excludes_orchestrator_only_skills() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"[
                {"name":"orchestration","seat":"orchestrator","summary":"Plan a goal.","document":"","document_is_default":true,"builtin":true,"created_at":"","updated_at":""},
                {"name":"coding","seat":"task","summary":"Write code.","document":"","document_is_default":true,"builtin":true,"created_at":"","updated_at":""}
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
        assert_eq!(skills[0]["name"], serde_json::json!("coding"));
    }

    /// An author submits its work in one request, and the summary travels
    /// as the transition's reason: it is the whole of what the reviewers are
    /// told, so nothing may be written anywhere else for them to have to
    /// find.
    #[tokio::test]
    async fn a_review_request_is_one_transition_carrying_the_summary() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.request_review(Parameters(RequestReviewReq {
            summary: "Rewrote the parser; cargo test green.".into(),
        }))
        .await
        .expect("submit for review");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/v1/tasks/01TASK/transitions");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(sent["to"], serde_json::json!("under_review"));
        assert_eq!(
            sent["reason"],
            serde_json::json!("Rewrote the parser; cargo test green.")
        );
    }

    /// Giving a task up moves it to `failed` with the reason on it, which is
    /// all the user is ever told; a reason with nothing in it is refused here
    /// rather than recorded, since a failed task saying nothing says nothing.
    #[tokio::test]
    async fn giving_a_task_up_records_the_reason_on_it() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpSeat::Author,
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
                McpSeat::Author,
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

    /// Reading a task is one round trip: the daemon names the profiles on it,
    /// so nothing here fetches the goal and the profile list to spell them. An
    /// agent reads its task on every wake-up.
    #[tokio::test]
    async fn reading_a_task_asks_the_daemon_once() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpSeat::Author,
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

    /// A verdict is a message to the author like any other, and what makes it
    /// close a round is its kind. So it goes to the channel, addressed to the
    /// agent the task's own staffing names as its author.
    #[tokio::test]
    async fn a_verdict_is_a_message_to_the_author_of_the_kind_that_closes_a_round() {
        for (verdict, word) in [
            (Verdict::Approve, "approve"),
            (Verdict::RequestChanges, "request_changes"),
        ] {
            let (endpoint, seen) = recording_daemon_answering(
                r#"{"agents":[{"id":"01AUTHOR","seat":"author","skills":["coding"]}]}"#,
            )
            .await;
            let mcp = server_at(
                McpSeat::Reviewer,
                Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
            );
            mcp.submit_verdict(Parameters(SubmitVerdictReq {
                verdict,
                body: Some("rebase first".into()),
            }))
            .await
            .expect("verdict");

            let seen = seen.lock().expect("lock").clone();
            // One read of the task to find its author, then the message.
            let sent = seen.last().expect("the verdict");
            assert_eq!(sent.method, "POST");
            assert_eq!(sent.path, "/v1/tasks/01TASK/messages");
            let body: serde_json::Value = serde_json::from_str(&sent.body).expect("json");
            assert_eq!(body["kind"], serde_json::json!(word));
            assert_eq!(body["to_actor"], serde_json::json!("author"));
            assert_eq!(body["to_agent_id"], serde_json::json!("01AUTHOR"));
            assert_eq!(body["body"], serde_json::json!("rebase first"));
        }
    }

    /// One verb, and it names the agent it is for.
    ///
    /// There is nothing to ask with and nothing to answer with: a message is
    /// one agent telling another what it needs from it, and each one arrives
    /// in a pane as a turn — a channel that invites one back spends two turns
    /// saying nothing.
    #[tokio::test]
    async fn a_message_names_the_agent_it_is_for() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"{"agents":[{"id":"01AUTHOR","seat":"author","skills":["coding"]},
                          {"id":"01REVIEWER","seat":"reviewer","skills":["code-review"]}]}"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Reviewer,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );

        mcp.send_message(Parameters(SendMessageReq {
            to: "01AUTHOR".into(),
            body: "The retry is bounded by the caller, so the inner one is not.".into(),
            task_id: None,
        }))
        .await
        .expect("send_message");
        let sent: serde_json::Value =
            serde_json::from_str(&seen.lock().expect("lock").last().expect("sent").body)
                .expect("json");
        assert_eq!(sent["kind"], serde_json::json!("message"));
        assert_eq!(sent["to_actor"], serde_json::json!("author"));
        assert_eq!(sent["to_agent_id"], serde_json::json!("01AUTHOR"));
    }

    /// The orchestrator is addressed by what it is: a goal has one, and it is
    /// staffed on no task, so there is no id to name it by.
    #[tokio::test]
    async fn the_orchestrator_is_addressed_by_name_and_needs_no_agent_id() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"{"agents":[{"id":"01AUTHOR","seat":"author","skills":["coding"]}]}"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Author,
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

    /// A `to` that names nobody is refused here, with the ids that would have
    /// worked: the agent reading the refusal is the one that has to fix it.
    #[tokio::test]
    async fn a_message_to_nobody_is_refused_with_the_addresses_that_would_work() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"{"agents":[{"id":"01AUTHOR","seat":"author","skills":["coding"]}]}"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Reviewer,
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
        assert!(err.message.contains("01AUTHOR"), "{}", err.message);
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

    /// The body of a change request is what the author is resumed with, so
    /// one with nothing in it is refused here rather than sent: a review that
    /// asks for changes and says nothing asks for nothing.
    #[tokio::test]
    async fn a_change_request_with_nothing_in_it_is_refused_before_it_is_sent() {
        for body in [None, Some(String::new()), Some("  \n ".into())] {
            let err = verdict_message(Verdict::RequestChanges, body.clone())
                .expect_err("empty change request");
            assert!(err.message.contains("needs a body"), "{}", err.message);
            assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);

            let (endpoint, seen) = recording_daemon().await;
            let mcp = server_at(
                McpSeat::Reviewer,
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

        // An approval carries a note or, where the reviewer wrote none, the
        // one word that says what it is: a message with nothing in it is not
        // one a pane can be handed.
        let approved = verdict_message(Verdict::Approve, None).expect("approval");
        assert_eq!(approved.kind, MessageKind::Approve);
        assert_eq!(approved.body, "Approved.");
    }

    /// What an orchestrator may write per agent is the schema an agent reads,
    /// and it is skills and a pin per agent now: a staffing object carrying
    /// what that agent knows and what it runs on. An agent that still sent the
    /// old author/reviewer profile fields would have staffed nothing at all,
    /// so those must be gone rather than merely ignored.
    #[test]
    fn the_task_tools_ask_for_skills_and_a_pin_per_agent() {
        for tool in ["create_task", "update_task"] {
            let schema = tool_schema(tool);
            let props = schema["properties"].as_object().expect("properties");
            assert!(props.contains_key("reviewers"), "{tool} takes no reviewers");
            for gone in ["author_profile", "reviewer_profiles"] {
                assert!(!props.contains_key(gone), "{tool} still takes {gone}");
            }
            let agent = schema["$defs"]["AgentReq"]["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{tool} has no agent object"));
            for field in ["skills", "model", "effort", "brief"] {
                assert!(agent.contains_key(field), "{tool}: no agent {field}");
            }
        }

        // Only a create staffs the author: a task keeps the one it started
        // with, so an edit offers the reviewers and the author's pin alone.
        let create = tool_schema("create_task");
        assert!(create["properties"].get("author").is_some());
        let update = tool_schema("update_task");
        assert!(update["properties"].get("author").is_none());
        for pin in ["author_model", "author_effort"] {
            assert!(
                update["properties"].get(pin).is_some(),
                "no {pin} on an edit"
            );
        }
    }

    /// A pin the orchestrator named is the pin the daemon is asked for, slot by
    /// slot: whatever this passes on is what the task is cut at, and a field
    /// quietly left out here is a task running on something nobody chose.
    #[tokio::test]
    async fn a_created_task_is_pinned_to_what_the_orchestrator_named() {
        let (endpoint, seen) = recording_daemon().await;
        orchestrator_at(&endpoint)
            .create_task(Parameters(CreateTaskReq {
                title: "Pin the effort".into(),
                description: "Beside the model.".into(),
                author: AgentReq {
                    skills: vec!["coding".into()],
                    model: "codex:gpt-5.6-sol".into(),
                    effort: Some("xhigh".into()),
                    brief: None,
                },
                reviewers: vec![AgentReq {
                    skills: vec!["code-review".into()],
                    model: "claude_code:claude-haiku-4-5".into(),
                    effort: Some("low".into()),
                    brief: None,
                }],
                depends_on: None,
                repo_id: None,
                landing: None,
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
                    "seat": "author",
                    "skills": ["coding"],
                    "model": "codex:gpt-5.6-sol",
                    "effort": "xhigh",
                    "brief": null,
                },
                {
                    "seat": "reviewer",
                    "skills": ["code-review"],
                    "model": "claude_code:claude-haiku-4-5",
                    "effort": "low",
                    "brief": null,
                },
            ])
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
                author_model: None,
                author_effort: Some("default".into()),
                reviewers: Some(vec![AgentReq {
                    skills: vec!["code-review".into()],
                    model: "codex:gpt-5.6-luna".into(),
                    effort: None,
                    brief: None,
                }]),
                depends_on: None,
                landing: None,
            }))
            .await
            .expect("edit the task");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "PATCH");
        assert_eq!(seen[0].path, "/v1/tasks/01TASK");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(sent["effort"], serde_json::json!("default"));
        assert_eq!(sent["model"], serde_json::Value::Null, "left alone");
        assert_eq!(
            sent["reviewers"],
            serde_json::json!([{
                "seat": "reviewer",
                "skills": ["code-review"],
                "model": "codex:gpt-5.6-luna",
                "effort": null,
                "brief": null,
            }])
        );

        let (endpoint, seen) = recording_daemon().await;
        let err = orchestrator_at(&endpoint)
            .update_task(Parameters(UpdateTaskReq {
                task_id: "01TASK".into(),
                title: None,
                description: None,
                author_model: Some("default".into()),
                author_effort: None,
                reviewers: None,
                depends_on: None,
                landing: None,
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
    /// that effort buys — and an agent kind narrows the answer rather than
    /// the question, since `GET /v1/models` takes no filter.
    #[tokio::test]
    async fn the_catalog_reaches_the_orchestrator_with_the_efforts_on_it() {
        const CATALOG: &str = r#"[
            {"id": "codex:gpt-5.6-sol", "agent_kind": "codex",
             "description": "frontier", "tier": "frontier", "cost": 4, "speed": 2,
             "best_for": ["cross-subsystem design"], "avoid_for": ["small fixes"],
             "efforts": [
               {"id": "low", "description": "lighter reasoning", "default": false},
               {"id": "high", "description": "greater depth", "default": true},
               {"id": "xhigh", "description": "deeper still", "default": false}
             ]},
            {"id": "claude_code:claude-haiku-4-5", "agent_kind": "claude_code",
             "description": "cheap", "tier": "fast", "cost": 2, "speed": 5,
             "best_for": ["inline edits"], "avoid_for": ["cross-subsystem design"],
             "efforts": []}
        ]"#;
        for (filter, ids) in [
            (
                None,
                vec!["codex:gpt-5.6-sol", "claude_code:claude-haiku-4-5"],
            ),
            (Some("codex"), vec!["codex:gpt-5.6-sol"]),
            (Some("opencode"), vec![]),
        ] {
            let (endpoint, seen) = recording_daemon_answering(CATALOG).await;
            let answered = orchestrator_at(&endpoint)
                .list_models(Parameters(ListModelsReq {
                    agent_kind: filter.map(str::to_string),
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
                assert_eq!(models[0]["tier"], serde_json::json!("frontier"));
                assert_eq!(models[0]["cost"], serde_json::json!(4));
                assert_eq!(
                    models[0]["best_for"],
                    serde_json::json!(["cross-subsystem design"])
                );
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
}
