//! Uniform API error shape: `{"error": {"code": "...", "message": "..."}}`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Wire envelope for every non-2xx response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorDetail {
    /// Stable machine-readable code, e.g. `task_not_found`, `illegal_transition`.
    #[schema(example = "task_not_found")]
    pub code: String,
    /// Human-readable description.
    pub message: String,
    /// Structured detail of the refusal, where the code carries more than a
    /// sentence — `workflow_invalid`'s `line`, for one. Boxed so the common
    /// case, no details, costs the error envelope one pointer rather than
    /// the size of the largest `serde_json::Value` variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Box<serde_json::Value>>,
}

impl ErrorBody {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                code: code.into(),
                message: message.into(),
                details: None,
            },
        }
    }

    pub fn with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            error: ErrorDetail {
                code: code.into(),
                message: message.into(),
                details: Some(Box::new(details)),
            },
        }
    }
}
