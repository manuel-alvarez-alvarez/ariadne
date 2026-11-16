//! The host list (037).
use super::AppState;
use super::error::{ApiResult, Json};
use crate::config::Config;
use ariadne_api::hosts::{HostDto, HostKind, HostStatus};

#[utoipa::path(get, path="/v1/hosts", tag="hosts", responses((status=200,body=[HostDto])))]
pub(super) async fn list(
    axum::extract::State(s): axum::extract::State<AppState>,
) -> ApiResult<Json<Vec<HostDto>>> {
    Ok(Json(host_list(&s.launcher.cfg)))
}

/// `local` first, then the configured hosts in file order.
fn host_list(config: &Config) -> Vec<HostDto> {
    let local = HostDto {
        name: "local".into(),
        kind: HostKind::Local,
        endpoint: Some(match config.tcp_listen {
            Some(address) => format!("http://{address}"),
            None => config.socket_path.display().to_string(),
        }),
        status: HostStatus::Ready,
        error: None,
        remote: None,
    };
    let configured = config.hosts.iter().map(|(name, host)| {
        if host.endpoint.starts_with("ssh://") {
            // No forward is started yet: the one that will be is a later task's.
            HostDto {
                name: name.clone(),
                kind: HostKind::Ssh,
                endpoint: None,
                status: HostStatus::Down,
                error: Some("ssh forward not started".into()),
                remote: host.remote.clone(),
            }
        } else {
            HostDto {
                name: name.clone(),
                kind: HostKind::Direct,
                endpoint: Some(host.endpoint.clone()),
                status: HostStatus::Ready,
                error: None,
                remote: None,
            }
        }
    });
    std::iter::once(local).chain(configured).collect()
}
