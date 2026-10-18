//! Dashboard DTOs.
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardDto {
    pub name: String,
    pub builtin: bool,
    pub document_is_default: bool,
    pub document: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDashboardRequest {
    pub name: String,
    pub document: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateDashboardRequest {
    pub document: serde_json::Value,
}
