//! Live issues of enabled forge repositories.

use axum::extract::{Path, Query, State};
use serde::Deserialize;
use utoipa::IntoParams;

use ariadne_api::issues::IssueDto;

use super::AppState;
use super::error::{ApiError, ApiResult, Json};
use crate::forge::ForgeClient;

#[derive(Debug, Default, Deserialize, IntoParams)]
pub(super) struct IssueListQuery {
    /// `me` filters to the forge login; `all` reads all open issues.
    #[param(value_type = Option<String>)]
    assigned: Option<String>,
}

async fn client(state: &AppState, id: &str) -> ApiResult<(ForgeClient, String, String)> {
    let repository = state.store.get_repository(id).await?;
    let forge = repository
        .forge
        .as_ref()
        .filter(|forge| forge.enabled)
        .ok_or_else(|| ApiError::conflict("the repository's forge integration is off"))?;
    Ok((
        ForgeClient::for_repository(&state.launcher.cfg, forge),
        format!("{}/{}", forge.owner, forge.name),
        forge.login.clone().unwrap_or_default(),
    ))
}

#[utoipa::path(get, path = "/v1/repositories/{id}/issues", tag = "issues",
    params(("id" = String, Path), IssueListQuery),
    responses((status = 200, body = [IssueDto]), (status = 409)))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<IssueListQuery>,
) -> ApiResult<Json<Vec<IssueDto>>> {
    let assigned = query.assigned.as_deref().unwrap_or("me");
    if !matches!(assigned, "me" | "all") {
        return Err(ApiError::bad_request("assigned must be me or all"));
    }
    let (client, repository, login) = client(&state, &id).await?;
    let assignee = (assigned == "me").then_some(login.as_str());
    let issues = client
        .list_open_issues(&repository, assignee)
        .await
        .map_err(|error| {
            ApiError::new(axum::http::StatusCode::BAD_GATEWAY, "forge_error", error)
        })?;
    Ok(Json(issues))
}

#[utoipa::path(get, path = "/v1/repositories/{id}/issues/{number}", tag = "issues",
    params(("id" = String, Path), ("number" = i64, Path)),
    responses((status = 200, body = IssueDto), (status = 409)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path((id, number)): Path<(String, i64)>,
) -> ApiResult<Json<IssueDto>> {
    let (client, repository, _) = client(&state, &id).await?;
    let issue = client.issue(&repository, number).await.map_err(|error| {
        ApiError::new(axum::http::StatusCode::BAD_GATEWAY, "forge_error", error)
    })?;
    Ok(Json(issue))
}
