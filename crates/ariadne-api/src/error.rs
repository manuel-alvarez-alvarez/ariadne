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
}

impl ErrorBody {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}
