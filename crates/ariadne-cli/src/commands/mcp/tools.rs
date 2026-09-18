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
use ariadne_api::knowledge::{
    KnowledgeDetail, KnowledgeHitDto, KnowledgeImpactDto, KnowledgeImpactQuery,
    KnowledgeOutlineEntryDto, KnowledgeOutlineQuery, KnowledgeRelatedDto, KnowledgeSearchQuery,
    KnowledgeSymbolDto, KnowledgeSymbolQuery,
};
use ariadne_api::memories::{CreateMemoryRequest, MemoryDto};
use ariadne_api::messages::SendMessageRequest;
use ariadne_api::skills::{SkillDto, SkillSeat};
use ariadne_api::tasks::{
    AgentAssignment, CreateTaskRequest, PickWinnerRequest, RecordPullRequestRequest,
    TransitionRequest, UpdateTaskRequest,
};
use ariadne_core::{Actor, Landing, MessageKind, PermissionMode, Seat, TaskStatus};

use super::{AriadneMcp, McpSeat, json_result, to_mcp_err};
use crate::commands::query_path;

/// How long a knowledge answer may be, in bytes. Codex cuts a tool result
/// at 10 KiB, so an answer is cut here first, with a last line that says
/// what was left out.
pub const ANSWER_CAP: usize = 8 * 1024;

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
    /// What it runs on, `<agent>:<model>` as `list_models` spells it.
    /// Required: every agent names its agent and its model.
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
    /// The agents that write the task, at least one. Most tasks take one.
    /// Staff several, each on its own model, where the task is worth two
    /// attempts: each writes it alone, and the reviewers pick the one change
    /// that lands. Several authors need at least one reviewer.
    pub authors: Vec<AgentReq>,
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
    /// How ACP permission requests run for this task. `auto` approves them,
    /// `ask` waits for a console answer, and `learn` remembers approvals in
    /// this repository. Omit it for the daemon default.
    pub permission_mode: Option<PermissionModeReq>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct UpdateTaskReq {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// What the author runs on, `<agent>:<model>`. Omit it to keep the
    /// model it has; a model is required, so `default` is refused. Refused
    /// on a task with several authors: replace them with `authors`.
    pub author_model: Option<String>,
    /// An `efforts[].id` for that model. `default` puts it back on the
    /// default effort.
    pub author_effort: Option<String>,
    /// The authors, in order. This list replaces the whole list, each author
    /// staffed afresh with the skills and the model it names.
    pub authors: Option<Vec<AgentReq>>,
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
    /// Filter: an `agent_id` as `list_models` gives it.
    pub agent_id: Option<String>,
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

/// The permission policy for one task.
///
/// Spelled the same way twice on purpose: the schema an agent reads is
/// `schemars`' and the value it sends back is `serde`'s, so a rename on one
/// alone advertises `auto` and then refuses it.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(crate = "rmcp::schemars", rename_all = "snake_case")]
pub enum PermissionModeReq {
    Auto,
    Ask,
    Learn,
}

impl From<PermissionModeReq> for PermissionMode {
    fn from(req: PermissionModeReq) -> PermissionMode {
        match req {
            PermissionModeReq::Auto => PermissionMode::Auto,
            PermissionModeReq::Ask => PermissionMode::Ask,
            PermissionModeReq::Learn => PermissionMode::Learn,
        }
    }
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
    /// The id of the author whose change you judge, from `get_task`.
    /// Required where the task has several authors; omit it where it has
    /// one.
    pub author: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct GetDiffReq {
    /// The id of the author whose branch to read, from `get_task`. Omit it
    /// where the task has one author.
    pub author: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct PickWinnerReq {
    /// The id of the author you pick, from `get_task`.
    pub author: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SendMessageReq {
    /// Who to write to: the id of an agent `get_task` lists, or
    /// `orchestrator`. A seat word works too: `author` or `reviewer` where
    /// the task staffs one.
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
pub struct ReadMessagesReq {
    /// The task whose channel to read. Omit it for your own task.
    pub task_id: Option<String>,
    /// Read the whole thread. Omit it to read only what is new for you.
    pub all: Option<bool>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SaveMemoryReq {
    /// Repository id. Omit it when this session works in one repository.
    pub repository_id: Option<String>,
    /// The useful fact to save.
    pub text: String,
    /// The RFC 3339 time after which this fact stays hidden.
    pub expires_at: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchMemoryReq {
    /// Repository id. Omit it when this session works in one repository.
    pub repository_id: Option<String>,
    /// The text to find, without case sensitivity.
    pub query: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchCodeReq {
    /// The name to find. Words, camelCase parts and snake_case parts match,
    /// each as a prefix.
    pub query: String,
    /// Repository id. Omit it for the repositories of your goal.
    pub repository: Option<String>,
    /// Set it to search every registered repository.
    pub all: Option<bool>,
    /// The branch to read. Omit it for your own branch.
    pub git_ref: Option<String>,
    /// Only this kind: `function`, `method`, `class`, `module`,
    /// `interface`, `macro`, `constant`, `test` or `heading`.
    pub kind: Option<String>,
    /// Only paths that contain this text.
    pub path: Option<String>,
    /// How many results at most: 20 by default, 50 at most.
    pub limit: Option<u32>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct OutlineReq {
    /// The path of the file, relative to the repository root.
    pub path: String,
    /// Repository id. Omit it when this session works in one repository.
    pub repository: Option<String>,
    /// The branch to read. Omit it for your own branch.
    pub git_ref: Option<String>,
}

/// How much `symbol` answers with. Spelled here because the schema an agent
/// reads is derived from the parameter types of this file.
#[derive(Clone, Copy, Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(crate = "rmcp::schemars", rename_all = "snake_case")]
pub enum DetailReq {
    /// The definition, its line range and its signature.
    Outline,
    /// The text of the definition too.
    Source,
    /// Its callers, callees, implementations and tests too.
    Context,
}

impl From<DetailReq> for KnowledgeDetail {
    fn from(req: DetailReq) -> KnowledgeDetail {
        match req {
            DetailReq::Outline => KnowledgeDetail::Outline,
            DetailReq::Source => KnowledgeDetail::Source,
            DetailReq::Context => KnowledgeDetail::Context,
        }
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SymbolReq {
    /// The name of the definition, spelled in full.
    pub name: String,
    /// Repository id. Omit it when this session works in one repository.
    pub repository: Option<String>,
    /// The branch to read. Omit it for your own branch.
    pub git_ref: Option<String>,
    /// How much to answer with: `outline` by default.
    pub detail: Option<DetailReq>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ImpactReq {
    /// The name of the definition you changed. Pass this or `diff`, never
    /// both.
    pub symbol: Option<String>,
    /// `<base>..<head>`: every definition the diff changed. A reviewer that
    /// passes neither reads the diff of its own task.
    pub diff: Option<String>,
    /// Repository id. Omit it when this session works in one repository.
    pub repository: Option<String>,
    /// The branch to read. Omit it for your own branch.
    pub git_ref: Option<String>,
    /// How far to walk the callers: 2 by default, 4 at most.
    pub depth: Option<u32>,
}

// ---------- helpers ----------

/// `lines` as one text under [`ANSWER_CAP`]: what fits, then a last line
/// that says how many results were left out.
fn cut_answer(lines: Vec<String>, cap: usize) -> String {
    // Room kept for the last line, whatever number it carries.
    const TAIL: usize = 64;
    let mut answer = String::new();
    for (at, line) in lines.iter().enumerate() {
        if answer.len() + line.len() + 1 > cap.saturating_sub(TAIL) {
            answer.push_str(&format!(
                "{} results left. Narrow the query.\n",
                lines.len() - at
            ));
            return answer;
        }
        answer.push_str(line);
        answer.push('\n');
    }
    answer
}

/// One end of an edge, as a knowledge answer prints it.
fn related_line(end: &KnowledgeRelatedDto) -> String {
    format!("{}:{} {} {}", end.path, end.line, end.name, end.confidence)
}

fn text_result(text: String) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

/// The query string of a knowledge request, as the daemon reads it.
fn knowledge_path(base: &str, query: &impl serde::Serialize) -> Result<String, McpError> {
    query_path(base, query).map_err(|e| McpError::invalid_params(e.to_string(), None))
}

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

/// Who a message is for, as an agent spells it: `orchestrator`, the id of an
/// agent the task staffs, or the seat word of a seat one agent sits in.
///
/// The seat is looked up rather than asked for. An agent reading `get_task`
/// has the ids in front of it and no reason to also work out which seat each
/// one sits in — and a `to` that named the wrong seat would be refused for a
/// reason nobody could act on.
///
/// A seat word is taken because most tasks staff one author and one reviewer,
/// and an agent that writes `author` on such a task means the only one there
/// is. Where the seat holds several, the word is ambiguous and is refused
/// with the ids of that seat.
fn addressee(to: &str, agents: &[serde_json::Value]) -> Result<(Actor, Option<String>), McpError> {
    if to.eq_ignore_ascii_case("orchestrator") {
        return Ok((Actor::Orchestrator, None));
    }
    let seated: Vec<&serde_json::Value> = agents
        .iter()
        .filter(|a| {
            a["seat"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(to))
        })
        .collect();
    let agent = match (agents.iter().find(|a| a["id"] == to), seated.as_slice()) {
        (Some(agent), _) => agent,
        (None, [only]) => only,
        (None, []) => {
            return Err(McpError::invalid_params(
                format!(
                    "no agent {to} on this task. Say `orchestrator`, or one of: {}",
                    roll_call(agents)
                ),
                None,
            ));
        }
        (None, several) => {
            return Err(McpError::invalid_params(
                format!(
                    "this task staffs several agents in the {to} seat. Name the one you \
                     write to: {}",
                    several
                        .iter()
                        .filter_map(|a| a["id"].as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None,
            ));
        }
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
    let Some(id) = agent["id"].as_str() else {
        return Err(McpError::invalid_params(
            format!("agent {to} has no id to write to"),
            None,
        ));
    };
    Ok((actor, Some(id.to_string())))
}

/// The addresses that would have worked, each id with the seat it sits in:
/// an agent reading a refusal picks its reader from this line.
fn roll_call(agents: &[serde_json::Value]) -> String {
    agents
        .iter()
        .filter_map(|a| Some((a["id"].as_str()?, a["seat"].as_str().unwrap_or("agent"))))
        .map(|(id, seat)| format!("{id} ({seat})"))
        .collect::<Vec<_>>()
        .join(", ")
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

    // ---- every seat ----

    #[tool(
        description = "Save a useful fact about this repository for later sessions. Use it for stable conventions, traps, or verification commands. Set when it expires."
    )]
    async fn save_memory(
        &self,
        Parameters(req): Parameters<SaveMemoryReq>,
    ) -> Result<CallToolResult, McpError> {
        let repository_id = self.memory_repository(req.repository_id).await?;
        let path = format!("/v1/repositories/{repository_id}/memories");
        json_result(
            self.post(
                &path,
                &CreateMemoryRequest {
                    text: req.text,
                    expires_at: req.expires_at,
                },
            )
            .await?,
        )
    }

    #[tool(
        description = "Search facts saved about this repository. Use it before repeated discovery, or when past work can answer a repository question."
    )]
    async fn search_memory(
        &self,
        Parameters(req): Parameters<SearchMemoryReq>,
    ) -> Result<CallToolResult, McpError> {
        let repository_id = self.memory_repository(req.repository_id).await?;
        let query = serde_urlencoded::to_string([("q", req.query)])
            .map_err(|error| McpError::invalid_params(error.to_string(), None))?;
        let path = format!("/v1/repositories/{repository_id}/memories/search?{query}");
        let memories: Vec<MemoryDto> = self.get(&path).await?;
        json_result(serde_json::to_value(memories).expect("memories serialize"))
    }

    #[tool(
        description = "Find a definition by name in the indexed code, on your own branch. Each line is `path:line kind name signature`. Narrow it with `kind`, `path` or `repository`."
    )]
    async fn search_code(
        &self,
        Parameters(req): Parameters<SearchCodeReq>,
    ) -> Result<CallToolResult, McpError> {
        let query = KnowledgeSearchQuery {
            q: req.query,
            repository: req.repository,
            all: req.all.filter(|all| *all),
            git_ref: req.git_ref,
            kind: req.kind,
            path: req.path,
            limit: req.limit.map(i64::from),
        };
        let hits: Vec<KnowledgeHitDto> = self
            .get(&knowledge_path("/v1/knowledge/search", &query)?)
            .await?;
        if hits.is_empty() {
            return text_result("No results.\n".into());
        }
        // Where the answer spans several repositories, each line says which
        // one its path is in.
        let several = hits
            .iter()
            .any(|hit| hit.repository_id != hits[0].repository_id);
        let lines = hits
            .iter()
            .map(|hit| {
                let location = format!(
                    "{}:{} {} {} {}",
                    hit.path, hit.line, hit.kind, hit.name, hit.signature
                );
                match several {
                    true => format!("{} {location}", hit.repository_id),
                    false => location,
                }
            })
            .collect();
        text_result(cut_answer(lines, ANSWER_CAP))
    }

    #[tool(
        description = "List the definitions of one file with their line ranges. Each line is `path:start-end kind name signature`. Read a function by its range instead of the whole file."
    )]
    async fn outline(
        &self,
        Parameters(req): Parameters<OutlineReq>,
    ) -> Result<CallToolResult, McpError> {
        let repository = self.memory_repository(req.repository).await?;
        let query = KnowledgeOutlineQuery {
            repository,
            path: req.path.clone(),
            git_ref: req.git_ref,
        };
        let entries: Vec<KnowledgeOutlineEntryDto> = self
            .get(&knowledge_path("/v1/knowledge/outline", &query)?)
            .await?;
        if entries.is_empty() {
            return text_result("No definitions.\n".into());
        }
        let lines = entries
            .iter()
            .map(|entry| {
                format!(
                    "{}:{}-{} {} {} {}",
                    req.path,
                    entry.start_line,
                    entry.end_line,
                    entry.kind,
                    entry.name,
                    entry.signature
                )
            })
            .collect();
        text_result(cut_answer(lines, ANSWER_CAP))
    }

    #[tool(
        description = "Read one definition by name, on your own branch: where it is, its signature and its doc. `detail=source` adds its text, and `detail=context` adds its callers, its callees, what implements it and the tests that reach it. Use it instead of grepping for a name and reading the files around it."
    )]
    async fn symbol(
        &self,
        Parameters(req): Parameters<SymbolReq>,
    ) -> Result<CallToolResult, McpError> {
        let repository = self.memory_repository(req.repository).await?;
        let query = KnowledgeSymbolQuery {
            name: req.name.clone(),
            repository: Some(repository),
            git_ref: req.git_ref,
            detail: req.detail.map(KnowledgeDetail::from),
        };
        let found: Vec<KnowledgeSymbolDto> = self
            .get(&knowledge_path("/v1/knowledge/symbol", &query)?)
            .await?;
        if found.is_empty() {
            return text_result(format!("No definition of {}.\n", req.name));
        }
        let mut lines = Vec::new();
        let mut repository = String::new();
        for definition in &found {
            // One heading per repository: what the answer holds for another
            // repository is under a heading of its own.
            if definition.repository_id != repository {
                repository = definition.repository_id.clone();
                lines.push(format!("# {repository}"));
            }
            lines.push(format!(
                "## {}:{}-{} {} {}",
                definition.path,
                definition.start_line,
                definition.end_line,
                definition.kind,
                definition.name
            ));
            lines.push(definition.signature.clone());
            if let Some(doc) = &definition.doc {
                lines.extend(doc.lines().map(str::to_string));
            }
            if let Some(source) = &definition.source {
                lines.push("### source".into());
                lines.extend(source.lines().map(str::to_string));
            }
            if let Some(context) = &definition.context {
                for (heading, ends) in [
                    ("callers", &context.callers),
                    ("callees", &context.callees),
                    ("implementations", &context.implementations),
                    ("tests", &context.tests),
                ] {
                    lines.push(format!("### {heading}"));
                    match ends.is_empty() {
                        true => lines.push("(none)".into()),
                        false => lines.extend(ends.iter().map(related_line)),
                    }
                }
            }
        }
        text_result(cut_answer(lines, ANSWER_CAP))
    }

    #[tool(
        description = "List what a change reaches: the callers of a definition, by how many calls away they are. Pass `symbol` for one definition, or `diff` as `<base>..<head>` for every definition a diff changed. A reviewer that passes neither reads the diff of its own task. Use it to see what one edit can break."
    )]
    async fn impact(
        &self,
        Parameters(req): Parameters<ImpactReq>,
    ) -> Result<CallToolResult, McpError> {
        let symbol = req.symbol.filter(|name| !name.trim().is_empty());
        let diff = req.diff.filter(|range| !range.trim().is_empty());
        // A reviewer that names nothing means the change it is judging.
        let (repository, diff) = match (&symbol, &diff) {
            (None, None) if self.seat == McpSeat::Reviewer => {
                let (repository, range) = self.task_diff().await?;
                (req.repository.unwrap_or(repository), Some(range))
            }
            (None, None) => {
                return Err(McpError::invalid_params(
                    "pass symbol, or diff as `<base>..<head>`",
                    None,
                ));
            }
            _ => (self.memory_repository(req.repository).await?, diff),
        };
        let query = KnowledgeImpactQuery {
            repository,
            git_ref: req.git_ref,
            symbol,
            diff,
            depth: req.depth.map(i64::from),
        };
        let found: Vec<KnowledgeImpactDto> = self
            .get(&knowledge_path("/v1/knowledge/impact", &query)?)
            .await?;
        if found.is_empty() {
            return text_result("No changed definition.\n".into());
        }
        let mut lines = Vec::new();
        let mut repository = String::new();
        for impact in &found {
            if impact.symbol.repository_id != repository {
                repository = impact.symbol.repository_id.clone();
                lines.push(format!("# {repository}"));
            }
            lines.push(format!(
                "## {}:{} {} — callers: {}",
                impact.symbol.path,
                impact.symbol.line,
                impact.symbol.name,
                impact.callers.len()
            ));
            for caller in &impact.callers {
                lines.push(format!(
                    "{} {}:{} {} {}",
                    caller.depth, caller.path, caller.line, caller.name, caller.confidence
                ));
            }
            for name in &impact.stopped {
                lines.push(format!(
                    "{name} has more than 200 callers: the walk stopped there."
                ));
            }
        }
        text_result(cut_answer(lines, ANSWER_CAP))
    }

    // ---- orchestrator ----

    #[tool(
        description = "Create one task in the goal. Staff its authors — one for most tasks, several to compare attempts — and the reviewers the user agreed it needs. Give each agent the skills its work needs (`list_skills`) and one model from `list_models`. Say how it ends with `landing`."
    )]
    async fn create_task(
        &self,
        Parameters(req): Parameters<CreateTaskReq>,
    ) -> Result<CallToolResult, McpError> {
        if req.authors.is_empty() {
            return Err(McpError::invalid_params(
                "a task takes at least one author: pass one entry in `authors`",
                None,
            ));
        }
        let path = format!("/v1/goals/{}/tasks", self.goal_id);
        let body = CreateTaskRequest {
            title: req.title,
            description: req.description,
            repo_id: req.repo_id,
            agents: req
                .authors
                .into_iter()
                .map(|a| assignment(Seat::Author, a))
                .chain(
                    req.reviewers
                        .into_iter()
                        .map(|r| assignment(Seat::Reviewer, r)),
                )
                .collect(),
            depends_on: req.depends_on.unwrap_or_default(),
            landing: req.landing.map(Into::into),
            permission_mode: req.permission_mode.map(Into::into),
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
            authors: req.authors.map(|authors| {
                authors
                    .into_iter()
                    .map(|a| assignment(Seat::Author, a))
                    .collect()
            }),
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
        description = "List the agents and models a slot can run on. Each entry gives:\n- its `id`, `<agent>:<model>`, and the description its agent gave\n- `efforts`, each an id and what it buys, one `default`"
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

    #[tool(
        description = "Read the diff of the branch under review against its base branch. On a task with several authors, pass `author` to say whose branch."
    )]
    async fn get_diff(
        &self,
        Parameters(req): Parameters<GetDiffReq>,
    ) -> Result<CallToolResult, McpError> {
        let mut path = self.task_path(None, "/diff")?;
        if let Some(author) = req.author {
            path.push_str(&format!("?agent={author}"));
        }
        // Plain-text endpoint: no JSON decoding.
        let diff = self.client.get_text(&path).await.map_err(to_mcp_err)?;
        Ok(CallToolResult::success(vec![ContentBlock::text(diff)]))
    }

    #[tool(
        description = "Give your verdict on the change. Approve it, or request changes. A change request carries the feedback the author starts again on. Where something blocks the review, request changes and name it. On a task with several authors, pass `author` to say whose change you judge."
    )]
    async fn submit_verdict(
        &self,
        Parameters(req): Parameters<SubmitVerdictReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.task_path(None, "/messages")?;
        let mut body = verdict_message(req.verdict, req.body)?;
        body.to_agent_id = Some(self.verdict_author(req.author).await?);
        json_result(self.post(&path, &body).await?)
    }

    #[tool(
        description = "Pick the author whose change lands, on a task with several authors. The pick opens once every author is approved. Call it once: a second pick is refused."
    )]
    async fn pick_winner(
        &self,
        Parameters(req): Parameters<PickWinnerReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.task_path(None, "/pick")?;
        json_result(
            self.post(
                &path,
                &PickWinnerRequest {
                    author_agent_id: req.author,
                },
            )
            .await?,
        )
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
        description = "Read the messages sent to you that you have not received yet, oldest first. Ariadne hands over each message once. Set `all` to true for the whole thread of the task or the goal. Read the whole thread to find the sha in the last verdict."
    )]
    async fn read_messages(
        &self,
        Parameters(req): Parameters<ReadMessagesReq>,
    ) -> Result<CallToolResult, McpError> {
        let path = self.channel_path(req.task_id, "/messages");
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

    /// The author a verdict is for: the one `named`, checked against the
    /// task's staffing, or the task's only author where none was — and a
    /// refusal naming the ids where the task has several and the verdict
    /// named none.
    async fn verdict_author(&self, named: Option<String>) -> Result<String, McpError> {
        let agents = self.agents_of(None).await?;
        let authors: Vec<&str> = agents
            .iter()
            .filter(|a| a["seat"] == "author")
            .filter_map(|a| a["id"].as_str())
            .collect();
        if let Some(named) = named {
            if authors.contains(&named.as_str()) {
                return Ok(named);
            }
            return Err(McpError::invalid_params(
                format!(
                    "no author {named} on this task; the authors are: {}",
                    authors.join(", ")
                ),
                None,
            ));
        }
        match authors.as_slice() {
            [author] => Ok((*author).to_string()),
            [] => Err(McpError::internal_error("this task has no author", None)),
            several => Err(McpError::invalid_params(
                format!(
                    "the task has several authors; pass `author` with the id of the one \
                     you judge: {}",
                    several.join(", ")
                ),
                None,
            )),
        }
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
    use crate::commands::mcp::tests::{
        recording_daemon, recording_daemon_answering, recording_daemon_answering_in_order,
        server_at,
    };

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

    /// Every seat uses these tools through the repository endpoints.
    #[tokio::test]
    async fn memory_tools_save_and_search_the_named_repository() {
        let (endpoint, seen) = recording_daemon().await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.save_memory(Parameters(SaveMemoryReq {
            repository_id: Some("01REPO".into()),
            text: "Run the parser fixture.".into(),
            expires_at: "2099-01-01T00:00:00Z".into(),
        }))
        .await
        .expect("save memory");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/v1/repositories/01REPO/memories");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(sent["text"], "Run the parser fixture.");
        assert_eq!(sent["expires_at"], "2099-01-01T00:00:00Z");

        let (endpoint, seen) = recording_daemon_answering("[]").await;
        let mcp = server_at(
            McpSeat::Reviewer,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.search_memory(Parameters(SearchMemoryReq {
            repository_id: Some("01REPO".into()),
            query: "parser fixture".into(),
        }))
        .await
        .expect("search memory");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "GET");
        assert_eq!(
            seen[0].path,
            "/v1/repositories/01REPO/memories/search?q=parser+fixture"
        );
    }

    #[tokio::test]
    async fn memory_tools_default_to_the_task_repository() {
        let (endpoint, seen) = recording_daemon_answering_in_order(&[
            r#"{"repo_id":"01REPO"}"#,
            "{}",
            r#"{"repo_id":"01REPO"}"#,
            "[]",
        ])
        .await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.save_memory(Parameters(SaveMemoryReq {
            repository_id: None,
            text: "Run the parser fixture.".into(),
            expires_at: "2099-01-01T00:00:00Z".into(),
        }))
        .await
        .expect("save memory");
        mcp.search_memory(Parameters(SearchMemoryReq {
            repository_id: None,
            query: "parser".into(),
        }))
        .await
        .expect("search memory");

        let seen = seen.lock().expect("lock").clone();
        let paths: Vec<&str> = seen.iter().map(|call| call.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/v1/tasks/01TASK",
                "/v1/repositories/01REPO/memories",
                "/v1/tasks/01TASK",
                "/v1/repositories/01REPO/memories/search?q=parser",
            ]
        );
    }

    #[tokio::test]
    async fn memory_search_defaults_to_the_goals_only_repository() {
        let (endpoint, seen) =
            recording_daemon_answering_in_order(&[r#"{"repos":[{"id":"01REPO"}]}"#, "[]"]).await;
        let mut mcp = server_at(
            McpSeat::Orchestrator,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.task_id = None;
        mcp.search_memory(Parameters(SearchMemoryReq {
            repository_id: None,
            query: "parser".into(),
        }))
        .await
        .expect("search memory");

        let seen = seen.lock().expect("lock").clone();
        let paths: Vec<&str> = seen.iter().map(|call| call.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/v1/goals/01GOAL",
                "/v1/repositories/01REPO/memories/search?q=parser",
            ]
        );
    }

    #[tokio::test]
    async fn memory_search_needs_a_repository_when_the_goal_has_several() {
        let (endpoint, seen) =
            recording_daemon_answering(r#"{"repos":[{"id":"01FIRST"},{"id":"01SECOND"}]}"#).await;
        let mut mcp = server_at(
            McpSeat::Orchestrator,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        mcp.task_id = None;
        let error = mcp
            .search_memory(Parameters(SearchMemoryReq {
                repository_id: None,
                query: "parser".into(),
            }))
            .await
            .expect_err("repository required");

        assert!(error.message.contains("pass repository_id"), "{error:?}");
        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].path, "/v1/goals/01GOAL");
    }

    /// `search_code` asks the daemon with every filter it was given, and
    /// answers one line per hit in the form `path:line kind name signature`.
    #[tokio::test]
    async fn search_code_asks_the_daemon_with_its_filters_and_answers_one_line_per_hit() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"[
                {"repository_id":"01REPO","path":"crates/ariadne-daemon/src/gitwt.rs","line":44,"kind":"method","name":"add_worktree","signature":"pub async fn add_worktree(&self, repo: &Path) -> Result<()>"},
                {"repository_id":"01REPO","path":"crates/ariadne-daemon/src/launcher.rs","line":9,"kind":"function","name":"add_worktree_later","signature":"fn add_worktree_later()"}
            ]"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let answered = mcp
            .search_code(Parameters(SearchCodeReq {
                query: "add_worktree".into(),
                repository: Some("01REPO".into()),
                all: None,
                git_ref: Some("main".into()),
                kind: Some("method".into()),
                path: Some("gitwt".into()),
                limit: Some(5),
            }))
            .await
            .expect("search code");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "GET");
        assert_eq!(
            seen[0].path,
            "/v1/knowledge/search?q=add_worktree&repository=01REPO&git_ref=main&kind=method&path=gitwt&limit=5"
        );
        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the answer is not text");
        };
        assert_eq!(
            text.text,
            "crates/ariadne-daemon/src/gitwt.rs:44 method add_worktree pub async fn add_worktree(&self, repo: &Path) -> Result<()>\n\
             crates/ariadne-daemon/src/launcher.rs:9 function add_worktree_later fn add_worktree_later()\n"
        );
    }

    /// Nothing but the query travels by default — the daemon knows the
    /// session's goal and branch — and `all` widens the search on request.
    #[tokio::test]
    async fn search_code_defaults_to_the_sessions_own_scope_and_widens_on_request() {
        let (endpoint, seen) = recording_daemon_answering("[]").await;
        let mcp = server_at(
            McpSeat::Reviewer,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let search = |all: Option<bool>| SearchCodeReq {
            query: "GitManager".into(),
            repository: None,
            all,
            git_ref: None,
            kind: None,
            path: None,
            limit: None,
        };
        let answered = mcp
            .search_code(Parameters(search(None)))
            .await
            .expect("search code");
        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the answer is not text");
        };
        assert_eq!(text.text, "No results.\n");
        mcp.search_code(Parameters(search(Some(false))))
            .await
            .expect("search code");
        mcp.search_code(Parameters(search(Some(true))))
            .await
            .expect("search code");

        let seen = seen.lock().expect("lock").clone();
        let paths: Vec<&str> = seen.iter().map(|call| call.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/v1/knowledge/search?q=GitManager",
                "/v1/knowledge/search?q=GitManager",
                "/v1/knowledge/search?q=GitManager&all=true",
            ]
        );
    }

    /// An answer over 8 KiB is cut, and its last line names how many results
    /// were left out and says to narrow the query.
    #[tokio::test]
    async fn an_answer_over_8_kib_is_cut_with_the_number_of_results_left() {
        let hits: Vec<serde_json::Value> = (0..300)
            .map(|n| {
                serde_json::json!({
                    "repository_id": "01REPO",
                    "path": format!("crates/ariadne-daemon/src/module_{n}.rs"),
                    "line": n,
                    "kind": "function",
                    "name": format!("symbol_{n}"),
                    "signature": format!("pub fn symbol_{n}(first: &str, second: usize) -> Result<Vec<String>>"),
                })
            })
            .collect();
        let answer: &'static str =
            Box::leak(serde_json::to_string(&hits).expect("json").into_boxed_str());
        let (endpoint, _) = recording_daemon_answering(answer).await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let answered = mcp
            .search_code(Parameters(SearchCodeReq {
                query: "symbol".into(),
                repository: None,
                all: None,
                git_ref: None,
                kind: None,
                path: None,
                limit: None,
            }))
            .await
            .expect("search code");
        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the answer is not text");
        };
        assert!(text.text.len() <= ANSWER_CAP, "{} bytes", text.text.len());
        let lines: Vec<&str> = text.text.lines().collect();
        let last = lines.last().expect("a last line");
        let kept = lines.len() - 1;
        assert_eq!(
            *last,
            format!("{} results left. Narrow the query.", 300 - kept)
        );
        assert!(kept > 50, "only {kept} lines fit");

        // A short answer is left whole.
        let short = cut_answer(vec!["a.rs:1 function a fn a()".into()], ANSWER_CAP);
        assert_eq!(short, "a.rs:1 function a fn a()\n");
    }

    /// `outline` takes the task's repository by default, like the memory
    /// tools, and answers one line per definition with its line range.
    #[tokio::test]
    async fn outline_defaults_to_the_task_repository_and_lists_line_ranges() {
        let (endpoint, seen) = recording_daemon_answering_in_order(&[
            r#"{"repo_id":"01REPO"}"#,
            r#"[{"kind":"class","name":"GitManager","start_line":17,"end_line":18,"signature":"pub struct GitManager"},
                {"kind":"method","name":"add_worktree","start_line":44,"end_line":63,"signature":"pub async fn add_worktree(&self)"}]"#,
        ])
        .await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let answered = mcp
            .outline(Parameters(OutlineReq {
                path: "crates/ariadne-daemon/src/gitwt.rs".into(),
                repository: None,
                git_ref: None,
            }))
            .await
            .expect("outline");

        let seen = seen.lock().expect("lock").clone();
        let paths: Vec<&str> = seen.iter().map(|call| call.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/v1/tasks/01TASK",
                "/v1/knowledge/outline?repository=01REPO&path=crates%2Fariadne-daemon%2Fsrc%2Fgitwt.rs",
            ]
        );
        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the answer is not text");
        };
        assert_eq!(
            text.text,
            "crates/ariadne-daemon/src/gitwt.rs:17-18 class GitManager pub struct GitManager\n\
             crates/ariadne-daemon/src/gitwt.rs:44-63 method add_worktree pub async fn add_worktree(&self)\n"
        );
    }

    /// `symbol` asks the daemon for the detail it was given, and groups its
    /// answer under a heading per repository, a heading per definition, and
    /// one per list of the context.
    #[tokio::test]
    async fn symbol_groups_its_answer_under_a_heading_for_each_repository() {
        let (endpoint, seen) = recording_daemon_answering(
            r#"[
                {"repository_id":"01REPO","path":"src/inner/m.rs","start_line":2,"end_line":2,
                 "kind":"function","name":"b","signature":"pub fn b()","doc":"Adds.",
                 "source":null,
                 "context":{"callers":[{"repository_id":"01REPO","path":"src/a.rs","line":3,"name":"a","confidence":"exact"}],
                            "callees":[],
                            "implementations":[],
                            "tests":[{"repository_id":"01REPO","path":"src/proof.rs","line":4,"name":"a_proof","confidence":"exact"}]}}
            ]"#,
        )
        .await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let answered = mcp
            .symbol(Parameters(SymbolReq {
                name: "b".into(),
                repository: Some("01REPO".into()),
                git_ref: Some("main".into()),
                detail: Some(DetailReq::Context),
            }))
            .await
            .expect("symbol");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(
            seen[0].path,
            "/v1/knowledge/symbol?name=b&repository=01REPO&git_ref=main&detail=context"
        );
        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the answer is not text");
        };
        assert_eq!(
            text.text,
            "# 01REPO\n\
             ## src/inner/m.rs:2-2 function b\n\
             pub fn b()\n\
             Adds.\n\
             ### callers\n\
             src/a.rs:3 a exact\n\
             ### callees\n\
             (none)\n\
             ### implementations\n\
             (none)\n\
             ### tests\n\
             src/proof.rs:4 a_proof exact\n"
        );
    }

    /// `symbol` takes the task's repository by default, like `outline` and the
    /// memory tools.
    #[tokio::test]
    async fn symbol_defaults_to_the_task_repository() {
        let (endpoint, seen) =
            recording_daemon_answering_in_order(&[r#"{"repo_id":"01REPO"}"#, "[]"]).await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let answered = mcp
            .symbol(Parameters(SymbolReq {
                name: "b".into(),
                repository: None,
                git_ref: None,
                detail: None,
            }))
            .await
            .expect("symbol");

        let seen = seen.lock().expect("lock").clone();
        let paths: Vec<&str> = seen.iter().map(|call| call.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/v1/tasks/01TASK",
                "/v1/knowledge/symbol?name=b&repository=01REPO"
            ]
        );
        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the answer is not text");
        };
        assert_eq!(text.text, "No definition of b.\n");
    }

    /// A reviewer that names neither a symbol nor a diff asks about its own
    /// task: the base branch of the repository to the task branch.
    #[tokio::test]
    async fn impact_reads_the_task_diff_for_a_reviewer_that_names_nothing() {
        let (endpoint, seen) = recording_daemon_answering_in_order(&[
            r#"{"repo_id":"01REPO","branch":"feat-task-abc"}"#,
            r#"{"id":"01REPO","base_branch":"main"}"#,
            r#"[{"symbol":{"repository_id":"01REPO","path":"src/inner/m.rs","line":2,"name":"b","confidence":"exact"},
                 "callers":[{"depth":1,"repository_id":"01REPO","path":"src/a.rs","line":3,"name":"a","confidence":"exact"}],
                 "stopped":["hot"]}]"#,
        ])
        .await;
        let mcp = server_at(
            McpSeat::Reviewer,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let answered = mcp
            .impact(Parameters(ImpactReq {
                symbol: None,
                diff: None,
                repository: None,
                git_ref: None,
                depth: None,
            }))
            .await
            .expect("impact");

        let seen = seen.lock().expect("lock").clone();
        let paths: Vec<&str> = seen.iter().map(|call| call.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/v1/tasks/01TASK",
                "/v1/repositories/01REPO",
                "/v1/knowledge/impact?repository=01REPO&diff=main..feat-task-abc",
            ]
        );
        let ContentBlock::Text(text) = &answered.content[0] else {
            panic!("the answer is not text");
        };
        assert_eq!(
            text.text,
            "# 01REPO\n\
             ## src/inner/m.rs:2 b — callers: 1\n\
             1 src/a.rs:3 a exact\n\
             hot has more than 200 callers: the walk stopped there.\n"
        );
    }

    /// Every other seat has to say what it is asking about.
    #[tokio::test]
    async fn impact_needs_a_symbol_or_a_diff_from_a_seat_that_is_no_reviewer() {
        let (endpoint, seen) = recording_daemon_answering("[]").await;
        let mcp = server_at(
            McpSeat::Author,
            Client::resolve(Some(&endpoint), None).with_session("01SESSION"),
        );
        let error = mcp
            .impact(Parameters(ImpactReq {
                symbol: None,
                diff: None,
                repository: None,
                git_ref: None,
                depth: None,
            }))
            .await
            .expect_err("symbol or diff required");

        assert!(error.message.contains("pass symbol, or diff"), "{error:?}");
        assert!(seen.lock().expect("lock").is_empty(), "nothing was asked");
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
                author: None,
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
    /// at its agent as a turn — a channel that invites one back spends two turns
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
                author: None,
            }))
            .await
            .expect_err("empty change request");
            assert!(seen.lock().expect("lock").is_empty());
        }

        // An approval carries a note or, where the reviewer wrote none, the
        // one word that says what it is: a message with nothing in it is not
        // one an agent can be handed.
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

        // Both tools staff the authors as a list — one for most tasks,
        // several for one the reviewers pick a winner on — and the old
        // one-author field is gone rather than merely ignored.
        for tool in ["create_task", "update_task"] {
            let schema = tool_schema(tool);
            assert!(schema["properties"].get("authors").is_some());
            assert!(schema["properties"].get("author").is_none());
        }
        // The one-author pin still moves on an edit without re-staffing.
        let update = tool_schema("update_task");
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
                authors: vec![AgentReq {
                    skills: vec!["coding".into()],
                    model: "codex-acp:gpt-5.6-sol".into(),
                    effort: Some("xhigh".into()),
                    brief: None,
                }],
                reviewers: vec![AgentReq {
                    skills: vec!["code-review".into()],
                    model: "claude-agent-acp:claude-haiku-4-5".into(),
                    effort: Some("low".into()),
                    brief: None,
                }],
                depends_on: None,
                repo_id: None,
                landing: None,
                permission_mode: Some(PermissionModeReq::Learn),
            }))
            .await
            .expect("create the task");

        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/v1/goals/01GOAL/tasks");
        let sent: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(sent["permission_mode"], "learn");
        assert_eq!(
            sent["agents"],
            serde_json::json!([
                {
                    "seat": "author",
                    "skills": ["coding"],
                    "model": "codex-acp:gpt-5.6-sol",
                    "effort": "xhigh",
                    "brief": null,
                },
                {
                    "seat": "reviewer",
                    "skills": ["code-review"],
                    "model": "claude-agent-acp:claude-haiku-4-5",
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
                authors: None,
                reviewers: Some(vec![AgentReq {
                    skills: vec!["code-review".into()],
                    model: "codex-acp:gpt-5.6-luna".into(),
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
                "model": "codex-acp:gpt-5.6-luna",
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
                authors: None,
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
    /// that effort buys — and an agent narrows the answer rather than
    /// the question, since `GET /v1/models` takes no filter.
    #[tokio::test]
    async fn the_catalog_reaches_the_orchestrator_with_the_efforts_on_it() {
        const CATALOG: &str = r#"[
            {"id": "codex-acp:gpt-5.6-sol", "agent_id": "codex-acp",
             "description": "frontier",
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

    /// A default read of the channel is a delivery: the daemon narrows it to
    /// this session's own agent and stamps what it hands over, so a message
    /// the agent has already had as a turn is not sent to it a second time as
    /// the whole thread. `all` is the whole thread, which a second review
    /// reads for the sha of the last verdict.
    #[tokio::test]
    async fn a_default_read_takes_delivery_and_all_reads_the_whole_thread() {
        for (all, path) in [
            (None, "/v1/tasks/01TASK/messages?deliver=true"),
            (Some(false), "/v1/tasks/01TASK/messages?deliver=true"),
            (Some(true), "/v1/tasks/01TASK/messages"),
        ] {
            let (endpoint, seen) = recording_daemon_answering("[]").await;
            server_at(
                McpSeat::Author,
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

    /// A seat word is an address where the seat holds one agent.
    ///
    /// Agents write `to: "author"` and `to: "reviewer"`, and a task with one
    /// of each leaves no doubt about who they mean. Refused, each of those
    /// cost a whole turn to say again with an id — so the word is taken, and
    /// the id it stands for is what travels.
    #[test]
    fn a_seat_word_addresses_the_one_agent_that_sits_in_it() {
        let agents = vec![
            serde_json::json!({"id": "01AUTHOR", "seat": "author"}),
            serde_json::json!({"id": "01REVIEWER", "seat": "reviewer"}),
        ];
        assert_eq!(
            addressee("author", &agents).expect("the one author"),
            (Actor::Author, Some("01AUTHOR".to_string()))
        );
        assert_eq!(
            addressee("reviewer", &agents).expect("the one reviewer"),
            (Actor::Reviewer, Some("01REVIEWER".to_string()))
        );
        // An id still addresses, and still travels as itself.
        assert_eq!(
            addressee("01REVIEWER", &agents).expect("by id"),
            (Actor::Reviewer, Some("01REVIEWER".to_string()))
        );

        // Where the seat holds several, the word means nobody in particular,
        // and the refusal names the ones it could have meant.
        let contested = vec![
            serde_json::json!({"id": "01FIRST", "seat": "author"}),
            serde_json::json!({"id": "01SECOND", "seat": "author"}),
        ];
        let err = addressee("author", &contested).expect_err("two authors");
        assert!(err.message.contains("01FIRST"), "{}", err.message);
        assert!(err.message.contains("01SECOND"), "{}", err.message);

        // And an address that is neither names the seats, so the sender can
        // pick a reader rather than guess again.
        let err = addressee("01NOBODY", &agents).expect_err("no such agent");
        assert!(err.message.contains("01AUTHOR (author)"), "{}", err.message);
        assert!(
            err.message.contains("01REVIEWER (reviewer)"),
            "{}",
            err.message
        );
    }

    /// The body of a message is taken under the name agents write it with.
    ///
    /// `message` is the commonest of those, and every one of them used to be
    /// refused as a missing `body` — a whole turn spent to send the same
    /// words under another key.
    #[test]
    fn a_message_body_is_taken_as_message_too() {
        let req: SendMessageReq = serde_json::from_value(serde_json::json!({
            "to": "01AUTHOR",
            "message": "The bound is the caller's.",
        }))
        .expect("a body written as `message`");
        assert_eq!(req.body, "The bound is the caller's.");
    }

    /// Every word the schema offers is a word the tool takes.
    ///
    /// The schema an agent reads is `schemars`', and the value it sends back
    /// is `serde`'s. A permission mode renamed on one of the two alone
    /// offered `auto` and then refused it as an unknown variant, which no
    /// agent reading the schema could have avoided.
    #[test]
    fn a_task_takes_every_permission_mode_its_schema_offers() {
        let schema = tool_schema("create_task");
        let offered = schema["$defs"]["PermissionModeReq"]["enum"]
            .as_array()
            .expect("the permission modes")
            .clone();
        assert_eq!(
            offered,
            ["auto", "ask", "learn"].map(|m| serde_json::json!(m))
        );
        for mode in offered {
            serde_json::from_value::<PermissionModeReq>(mode.clone()).unwrap_or_else(|e| {
                panic!("the schema offers {mode} and the tool refuses it: {e}")
            });
        }
    }
}
