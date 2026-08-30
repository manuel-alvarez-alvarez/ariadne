//! The tools themselves: what each one takes, and the request it makes.
//!
//! The parameter types are the wire contract — `schemars` derives the JSON
//! schema an agent reads from them, and the `#[tool]` descriptions are what it
//! chooses between — so a rename here is a rename an agent sees.
//!
//! Every tool is the same three steps: name the endpoint, send the request,
//! answer with what came back. Only the endpoint differs, and the two that
//! depend on the session rather than on the arguments go through
//! [`AriadneMcp::thread`] and [`AriadneMcp::task_path`].

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};

use ariadne_api::goals::FinalizePlanRequest;
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::reviews::CreateReviewRequest;
use ariadne_api::tasks::{
    CreateTaskRequest, RecordPullRequestRequest, ReviewerAssignment, TransitionRequest,
    UpdateTaskRequest,
};
use ariadne_core::{ReviewVerdict, TaskStatus};

use super::{AriadneMcp, json_result, to_mcp_err};

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
    /// Whom to wake, as the MCP instructions spell it; omit to leave the
    /// message in the thread.
    pub to: Option<String>,
}

/// One reviewer of a task as a planner names it: the profile that reviews,
/// and what this task is worth having it run on.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ReviewerReq {
    /// Reviewer profile id or name.
    pub profile: String,
    /// What it runs on, `<agent_kind>[:<model>]` as `list_models` spells it;
    /// omit for the profile's own.
    pub model: Option<String>,
    /// An `efforts[].id` `list_models` lists for that model; omit for its
    /// default.
    pub effort: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CreateTaskReq {
    pub title: String,
    pub description: String,
    /// Engineer profile id or name that will own the task.
    pub engineer_profile: String,
    /// What the engineer runs on, `<agent_kind>[:<model>]` as `list_models`
    /// spells it; omit for the profile's own.
    pub engineer_model: Option<String>,
    /// An `efforts[].id` `list_models` lists for that model; omit for its
    /// default.
    pub engineer_effort: Option<String>,
    /// The reviewers of the task, in review order (at least one).
    pub reviewers: Vec<ReviewerReq>,
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
    /// What the engineer runs on; "default" hands the slot back to the
    /// profile's own.
    pub engineer_model: Option<String>,
    /// An `efforts[].id` for that model; "default" hands it back to the
    /// model's own.
    pub engineer_effort: Option<String>,
    /// Full replacement list of the reviewers, in review order.
    pub reviewers: Option<Vec<ReviewerReq>>,
    /// Full replacement list of the ids of the tasks that must merge first.
    pub depends_on: Option<Vec<String>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListModelsReq {
    /// Filter: claude_code | codex | opencode
    pub agent_kind: Option<String>,
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
    /// An approval's note; on a change request, the feedback the engineer is
    /// resumed with, and required.
    pub body: Option<String>,
}

// ---------- helpers ----------

/// One message as an agent reads it: the DTO's nested recipient flattened into
/// the `to` that `post_message` takes, so a listing shows the word that
/// addresses a reply back.
fn addressed_message(message: &MessageDto) -> serde_json::Value {
    serde_json::json!({
        "id": message.id,
        "task_id": message.task_id,
        "author_role": message.author_role.as_str(),
        "author_session_id": message.author_session_id,
        "to": message.recipient.as_ref().map(crate::commands::recipient_label),
        "body": message.body,
        "created_at": message.created_at,
    })
}

/// The reviewer slot a planner named, as the API takes it: the profile, and
/// the pin it is to be cut at.
fn assignment(reviewer: ReviewerReq) -> ReviewerAssignment {
    ReviewerAssignment {
        profile: reviewer.profile,
        model: reviewer.model,
        effort: reviewer.effort,
    }
}

/// The catalog narrowed to one agent CLI, or all of it. Entries pass through
/// as the daemon wrote them: what a model is called and what it can be run at
/// is the daemon's answer, not this file's.
fn of_agent(models: Vec<serde_json::Value>, agent_kind: Option<String>) -> Vec<serde_json::Value> {
    let Some(kind) = agent_kind else {
        return models;
    };
    models
        .into_iter()
        .filter(|m| m["agent_kind"] == serde_json::Value::String(kind.clone()))
        .collect()
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

#[tool_router(vis = "pub(super)")]
impl AriadneMcp {
    #[tool(
        description = "Read a task: status, branch, dependencies, and the profile names of its engineer, its reviewers and the planner."
    )]
    async fn get_task(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        json_result(self.get(&self.task_path(req.task_id, "")?).await?)
    }

    #[tool(
        description = "Read a task's conversation, or the goal thread when a planner passes no task_id."
    )]
    async fn list_messages(
        &self,
        Parameters(req): Parameters<TaskIdOpt>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.thread(req.task_id, "/messages?limit=200");
        let messages: Vec<MessageDto> = self.get(&path).await?;
        json_result(messages.iter().map(addressed_message).collect())
    }

    #[tool(
        description = "Write a message into a task's conversation, or into the goal thread when a planner passes no task_id."
    )]
    async fn post_message(
        &self,
        Parameters(req): Parameters<PostMessageReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.thread(req.task_id, "/messages");
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
        description = "Create one task in the goal: one engineer profile, at least one reviewer profile, and per slot the model and effort this task deserves out of `list_models`. Omit either and the slot runs its profile's own; the user may override until the task starts."
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
            engineer_profile: req.engineer_profile,
            model: req.engineer_model,
            effort: req.engineer_effort,
            reviewers: req.reviewers.into_iter().map(assignment).collect(),
            depends_on: req.depends_on.unwrap_or_default(),
        };
        json_result(self.post(&path, &body).await?)
    }

    #[tool(
        description = "Edit a task that has not started: title, description, reviewers, dependencies, or any slot's model and effort. `reviewers` replaces the whole list; \"default\" hands a slot back to its profile's own."
    )]
    async fn update_task(
        &self,
        Parameters(req): Parameters<UpdateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        let body = UpdateTaskRequest {
            title: req.title,
            description: req.description,
            model: req.engineer_model,
            effort: req.engineer_effort,
            reviewers: req
                .reviewers
                .map(|reviewers| reviewers.into_iter().map(assignment).collect()),
            depends_on: req.depends_on,
        };
        let path = format!("/v1/tasks/{}", req.task_id);
        let value = self.client.patch_json(&path, &body).await;
        json_result(value.map_err(to_mcp_err)?)
    }

    #[tool(
        description = "Every agent CLI and model a slot can be pinned to, as a task takes it: a description; a `tier` (frontier down to fast, or `unknown`, with no bands or shapes); `cost` and `speed` 1-5, low to high and slow to fast; `best_for`/`avoid_for` shapes; `efforts`, each an id and what it buys, one `default`."
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
        description = "The agent profiles a task can be assigned to, each with the name, model, effort and system prompt that say what it is for."
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

    #[tool(
        description = "Finalize the plan and start its tasks: only after the user has confirmed in the thread that it is complete."
    )]
    async fn finalize_plan(
        &self,
        Parameters(req): Parameters<FinalizePlanReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = format!("/v1/goals/{}/finalize", self.goal_id);
        let body = FinalizePlanRequest {
            summary: req.summary,
        };
        json_result(self.post(&path, &body).await?)
    }

    // ---- engineer ----

    #[tool(description = "Submit your task for review, under the summary the reviewers read first.")]
    async fn request_review(
        &self,
        Parameters(req): Parameters<RequestReviewReq>,
    ) -> Result<CallToolResult, McpError> {
        // Summary first (reviewers read it), then the status transition.
        self.post(
            &self.task_path(None, "/messages")?,
            &CreateMessageRequest {
                body: format!("Review requested: {}", req.summary),
                to: None,
            },
        )
        .await?;
        json_result(
            self.transition(TaskStatus::UnderReview, Some(req.summary), None)
                .await?,
        )
    }

    #[tool(description = "Read the verdicts and feedback on your task, every round of them.")]
    async fn get_reviews(
        &self,
        Parameters(_): Parameters<Empty>,
    ) -> Result<CallToolResult, McpError> {
        json_result(self.get(&self.task_path(None, "/reviews")?).await?)
    }

    #[tool(
        description = "Report the sha your branch landed on its base branch as; this ends the task."
    )]
    async fn mark_merged(
        &self,
        Parameters(req): Parameters<MarkMergedReq>,
    ) -> Result<CallToolResult, McpError> {
        json_result(
            self.transition(TaskStatus::Merged, None, Some(req.merge_commit))
                .await?,
        )
    }

    #[tool(description = "Report the URL of the pull or merge request you opened for this task.")]
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
        description = "Your verdict on the change: approve it, or request changes with the feedback the engineer is resumed with."
    )]
    async fn submit_verdict(
        &self,
        Parameters(req): Parameters<SubmitVerdictReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.task_path(None, "/reviews")?;
        let body = review_request(req.verdict, req.body)?;
        json_result(self.post(&path, &body).await?)
    }
}

impl AriadneMcp {
    /// Move this session's own task, which is the only one an engineer may
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

    use ariadne_api::messages::MessageRecipientDto;
    use ariadne_client::Client;
    use ariadne_core::{AuthorRole, RecipientKind};

    use crate::commands::mcp::McpRole;
    use crate::commands::mcp::tests::{recording_daemon, recording_daemon_answering, server_at};

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

    /// A planner server against a daemon that records what it is sent.
    fn planner_at(endpoint: &str) -> AriadneMcp {
        server_at(
            McpRole::Planner,
            Client::resolve(Some(endpoint), None).with_session("01SESSION"),
        )
    }

    /// An agent reads a thread to know who was asked; the addressee it reads
    /// is spelled the way `post_message`'s `to` would address them back.
    #[test]
    fn a_listed_message_carries_the_word_that_addressed_it() {
        let message = |recipient| MessageDto {
            id: "01MSG".into(),
            goal_id: "01GOAL".into(),
            task_id: Some("01TASK".into()),
            author_role: AuthorRole::Reviewer,
            author_session_id: Some("01SESSION".into()),
            recipient,
            body: "rebase first".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let to_engineer = MessageRecipientDto {
            kind: RecipientKind::Profile,
            profile_id: Some("01PROF".into()),
            profile_name: Some("Engineer".into()),
        };
        let addressed = addressed_message(&message(Some(to_engineer)));
        assert_eq!(addressed["to"], serde_json::json!("Engineer"));
        assert_eq!(addressed["body"], serde_json::json!("rebase first"));
        assert_eq!(addressed["author_role"], serde_json::json!("reviewer"));
        assert_eq!(
            addressed_message(&message(None))["to"],
            serde_json::Value::Null
        );
    }

    /// Reading a task is one round trip: the daemon names the profiles on it,
    /// so nothing here fetches the goal and the profile list to spell them. An
    /// agent reads its task on every wake-up.
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

    /// One tool for both verdicts writes the row each of the two it replaced
    /// wrote: the same route, the same verdict word, the same body.
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

    /// What a planner may write per slot is the schema an agent reads, and it
    /// is a pin per slot now: the engineer's model and effort beside its
    /// profile, and a reviewer object carrying its own — not the list of bare
    /// profile names those replaced, which an agent that still sent one would
    /// have silently dropped its pins with.
    #[test]
    fn the_task_tools_ask_for_a_model_and_an_effort_per_slot() {
        for tool in ["create_task", "update_task"] {
            let schema = tool_schema(tool);
            let props = schema["properties"].as_object().expect("properties");
            for field in ["engineer_model", "engineer_effort", "reviewers"] {
                assert!(props.contains_key(field), "{tool} takes no {field}");
            }
            assert!(
                !props.contains_key("reviewer_profiles"),
                "{tool} still takes the bare profile list"
            );
            let reviewer = schema["$defs"]["ReviewerReq"]["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{tool} has no reviewer object"));
            for field in ["profile", "model", "effort"] {
                assert!(reviewer.contains_key(field), "{tool}: no reviewer {field}");
            }
        }
        assert!(
            tool_schema("create_task")["properties"]
                .get("engineer_profile")
                .is_some(),
            "the engineer's profile is still what owns the task"
        );
    }

    /// A pin the planner named is the pin the daemon is asked for, slot by
    /// slot: whatever this passes on is what the task is cut at, and a field
    /// quietly left out here is a task running on something nobody chose.
    #[tokio::test]
    async fn a_created_task_is_pinned_to_what_the_planner_named() {
        let (endpoint, seen) = recording_daemon().await;
        planner_at(&endpoint)
            .create_task(Parameters(CreateTaskReq {
                title: "Pin the effort".into(),
                description: "Beside the model.".into(),
                engineer_profile: "Engineer".into(),
                engineer_model: Some("codex:gpt-5.6-sol".into()),
                engineer_effort: Some("xhigh".into()),
                reviewers: vec![ReviewerReq {
                    profile: "Reviewer".into(),
                    model: None,
                    effort: Some("low".into()),
                }],
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
        assert_eq!(sent["engineer_profile"], serde_json::json!("Engineer"));
        assert_eq!(sent["model"], serde_json::json!("codex:gpt-5.6-sol"));
        assert_eq!(sent["effort"], serde_json::json!("xhigh"));
        assert_eq!(
            sent["reviewers"],
            serde_json::json!([{ "profile": "Reviewer", "model": null, "effort": "low" }])
        );
    }

    /// The word that hands a slot back to its profile travels as it was
    /// written: the daemon is what knows "default" clears a pin, so anything
    /// resolving it here would be a second answer to the same question.
    #[tokio::test]
    async fn an_edit_hands_a_slot_back_with_the_word_the_daemon_clears_it_by() {
        let (endpoint, seen) = recording_daemon().await;
        planner_at(&endpoint)
            .update_task(Parameters(UpdateTaskReq {
                task_id: "01TASK".into(),
                title: None,
                description: None,
                engineer_model: None,
                engineer_effort: Some("default".into()),
                reviewers: Some(vec![ReviewerReq {
                    profile: "Reviewer".into(),
                    model: Some("default".into()),
                    effort: None,
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
        assert_eq!(sent["effort"], serde_json::json!("default"));
        assert_eq!(sent["model"], serde_json::Value::Null);
        assert_eq!(
            sent["reviewers"],
            serde_json::json!([{ "profile": "Reviewer", "model": "default", "effort": null }])
        );
    }

    /// The catalog is what a planner sizes a task from, so it reaches it whole
    /// — what each model is for, and every effort it takes with what that
    /// effort buys — and an agent kind narrows the answer rather than the
    /// question, since `GET /v1/models` takes no filter.
    #[tokio::test]
    async fn the_catalog_reaches_the_planner_with_the_efforts_on_it() {
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
            let answered = planner_at(&endpoint)
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
