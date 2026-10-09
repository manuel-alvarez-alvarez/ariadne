//! Workflow DTOs.

use ariadne_core::models::ModelRank;
use ariadne_core::workflow::StepGate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One column of a workflow.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowStepDto {
    /// Kebab-case, unique within the workflow.
    #[schema(example = "develop")]
    pub id: String,
    /// The display title.
    #[schema(example = "Develop")]
    pub title: String,
    pub description: String,
    /// The skills an agent on this step loads.
    pub skills: Vec<String>,
    /// The model rank the step prefers, where the document named one.
    pub rank: Option<ModelRank>,
    /// What the step waits for before the next one starts, where the
    /// document named one.
    pub gate: Option<StepGate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowDto {
    /// Kebab-case; named on the document's first line.
    #[schema(example = "develop-review-merge")]
    pub name: String,
    /// The effective document: the override, or the text Ariadne ships.
    pub document: String,
    /// Whether Ariadne ships this workflow. A built-in is reset rather than
    /// deleted; a workflow of the user's own is deleted rather than reset.
    pub builtin: bool,
    /// `document`, parsed into its columns.
    pub steps: Vec<WorkflowStepDto>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkflowRequest {
    /// Kebab-case, and free: a name Ariadne already ships is refused.
    #[schema(example = "my-workflow")]
    pub name: String,
    /// The whole workflow document. Its `workflow <name>` line must equal
    /// `name`.
    pub document: String,
}

/// Partial update; absent fields stay unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkflowRequest {
    /// The new document. Absent = unchanged; putting a built-in back on the
    /// text Ariadne ships is `POST /v1/workflows/{name}/reset`.
    pub document: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ParseWorkflowRequest {
    pub document: String,
}

/// Response of `POST /v1/workflows/parse`: a document's name and columns,
/// read without saving it anywhere.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ParsedWorkflowDto {
    pub name: String,
    pub steps: Vec<WorkflowStepDto>,
}
