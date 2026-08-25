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
    CreateTaskRequest, RecordPullRequestRequest, TransitionRequest, UpdateTaskRequest,
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
        description = "Read a task: its status, its branch, its dependencies and the profile names of its engineer, its reviewers and the planner."
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
        description = "Create one task in the goal, owned by one engineer profile and gated by at least one reviewer profile."
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
            reviewer_profiles: req.reviewer_profiles,
            depends_on: req.depends_on.unwrap_or_default(),
        };
        json_result(self.post(&path, &body).await?)
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
        let path = format!("/v1/tasks/{}", req.task_id);
        let value = self.client.patch_json(&path, &body).await;
        json_result(value.map_err(to_mcp_err)?)
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
        let path = format!("/v1/goals/{}/finalize", self.goal_id);
        let body = FinalizePlanRequest {
            summary: req.summary,
        };
        json_result(self.post(&path, &body).await?)
    }

    // ---- engineer ----

    #[tool(description = "Submit your task for review, with the summary the reviewers read first.")]
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
        description = "Report the sha your branch landed on its base branch as, which ends the task."
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
        json_result(self.post(&path, &RecordPullRequestRequest { url: req.url }).await?)
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
        description = "Deliver your verdict on the change under review, approving it or requesting changes with the feedback the engineer is resumed with."
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
    use crate::commands::mcp::tests::{recording_daemon, server_at};

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
        assert_eq!(addressed_message(&message(None))["to"], serde_json::Value::Null);
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
}
