//! Dashboard endpoints.
use super::AppState;
use super::convert::dashboard_dto;
use super::error::{ApiError, ApiResult, Json};
use ariadne_api::dashboards::{CreateDashboardRequest, DashboardDto, UpdateDashboardRequest};
use ariadne_store::{NewDashboard, StoreError};
use axum::extract::{Path, State};
use axum::http::StatusCode;

#[utoipa::path(post, path="/v1/dashboards", tag="dashboards", request_body=CreateDashboardRequest, responses((status=201,body=DashboardDto),(status=409),(status=422)))]
pub(super) async fn create(
    State(s): State<AppState>,
    Json(r): Json<CreateDashboardRequest>,
) -> ApiResult<(StatusCode, Json<DashboardDto>)> {
    let d = s
        .store
        .create_dashboard(NewDashboard {
            name: r.name,
            document: r.document.to_string(),
        })
        .await
        .map_err(dashboard_error)?;
    Ok((StatusCode::CREATED, Json(dashboard_dto(d))))
}
#[utoipa::path(get, path="/v1/dashboards", tag="dashboards", responses((status=200,body=[DashboardDto])))]
pub(super) async fn list(State(s): State<AppState>) -> ApiResult<Json<Vec<DashboardDto>>> {
    Ok(Json(
        s.store
            .list_dashboards()
            .await?
            .into_iter()
            .map(dashboard_dto)
            .collect(),
    ))
}
#[utoipa::path(get, path="/v1/dashboards/{name}", tag="dashboards", responses((status=200,body=DashboardDto),(status=404)))]
pub(super) async fn get(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<DashboardDto>> {
    Ok(Json(dashboard_dto(
        s.store
            .get_dashboard(&name)
            .await
            .map_err(dashboard_error)?,
    )))
}
#[utoipa::path(put, path="/v1/dashboards/{name}", tag="dashboards", request_body=UpdateDashboardRequest, responses((status=200,body=DashboardDto),(status=422)))]
pub(super) async fn update(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(r): Json<UpdateDashboardRequest>,
) -> ApiResult<Json<DashboardDto>> {
    Ok(Json(dashboard_dto(
        s.store
            .set_dashboard_document(&name, &r.document.to_string())
            .await
            .map_err(dashboard_error)?,
    )))
}
#[utoipa::path(delete, path="/v1/dashboards/{name}", tag="dashboards", responses((status=204),(status=409)))]
pub(super) async fn delete(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    s.store
        .delete_dashboard(&name)
        .await
        .map_err(dashboard_error)?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(post, path="/v1/dashboards/{name}/reset", tag="dashboards", responses((status=200,body=DashboardDto),(status=409)))]
pub(super) async fn reset(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<DashboardDto>> {
    Ok(Json(dashboard_dto(
        s.store
            .reset_dashboard(&name)
            .await
            .map_err(dashboard_error)?,
    )))
}
fn dashboard_error(e: StoreError) -> ApiError {
    match e {
        StoreError::NotFound { .. } => {
            ApiError::new(StatusCode::NOT_FOUND, "dashboard_not_found", e.to_string())
        }
        StoreError::Invalid(_) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            e.to_string(),
        ),
        StoreError::DashboardBuiltin(name) => ApiError::new(
            StatusCode::CONFLICT,
            "dashboard_builtin",
            format!("dashboard {name} is one Ariadne ships; reset it instead"),
        ),
        other => other.into(),
    }
}
