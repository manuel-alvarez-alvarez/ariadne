//! Route source session aliases to their worker without local session copies.
use ariadne_api::sessions::SessionDto;
use ariadne_store::SourceTaskReference;
use axum::extract::{FromRequestParts, Request};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};

use super::{
    AppState,
    error::{ApiError, ApiResult},
};

pub(super) async fn route(
    state: &AppState,
    request: Request,
    reference: SourceTaskReference,
    session_id: &str,
    suffix: &str,
) -> ApiResult<Response> {
    let ctx = super::caller::call_ctx(&state.store, request.headers()).await?;
    super::caller::ensure_task_scope(&ctx, &reference.id)?;
    if ctx
        .session
        .as_ref()
        .is_some_and(|s| s.goal_id.as_deref() != Some(&reference.goal_id))
    {
        return Err(ApiError::forbidden(
            "session belongs to another source goal",
        ));
    }
    if request.method() != Method::GET
        && !matches!(
            ctx.actor,
            ariadne_core::Actor::User | ariadne_core::Actor::Orchestrator
        )
    {
        return Err(ApiError::forbidden(
            "only the source orchestrator or user may control remote sessions",
        ));
    }
    if request.method() != Method::GET
        && !matches!(suffix, "kill" | "console/cancel")
        && state.store.get_goal(&reference.goal_id).await?.status()
            == ariadne_core::GoalStatus::Cancelled
    {
        return Err(ApiError::conflict("the source goal is cancelled"));
    }
    let client = super::tasks::far_client(state, &reference.host)?;
    let session: SessionDto = client
        .get_json(&format!("/v1/sessions/{session_id}"))
        .await
        .map_err(|e| super::tasks::far_error(&reference.host, e))?;
    if session.task_id.as_deref() != Some(&reference.id) {
        return Err(ApiError::forbidden(
            "session does not belong to the referenced task",
        ));
    }
    let task: ariadne_api::tasks::TaskDto = client
        .get_json(&format!("/v1/tasks/{}", reference.id))
        .await
        .map_err(|e| super::tasks::far_error(&reference.host, e))?;
    if task.execution_context.as_ref().map(|c| &c.execution_owner) != Some(&reference.worker_id) {
        return Err(ApiError::conflict(
            "host names a different worker; restore the original worker",
        ));
    }
    let mut target = format!("/v1/sessions/{session_id}");
    if !suffix.is_empty() {
        target.push('/');
        target.push_str(suffix);
    }
    if let Some(query) = request.uri().query() {
        target.push('?');
        target.push_str(query);
    }
    if matches!(suffix, "console/stream" | "trajectory/stream") {
        let stream = client
            .stream(&target)
            .await
            .map_err(|e| super::tasks::far_error(&reference.host, e))?;
        let task_id = reference.id;
        let session_id = session_id.to_owned();
        let stream = futures_util::stream::unfold(
            (stream, task_id, session_id),
            |(mut stream, task, session)| async move {
                let frame = stream.next().await?;
                let frame = match frame {
                    Ok(frame) => frame,
                    Err(_) => return None,
                };
                let data = match serde_json::from_str::<serde_json::Value>(&frame.data) {
                    Ok(mut value) => {
                        rewrite(&mut value, &task, &session);
                        value.to_string()
                    }
                    Err(_) => frame.data,
                };
                let mut event = axum::response::sse::Event::default()
                    .event(frame.event)
                    .data(data);
                if let Some(id) = frame.id {
                    event = event.id(id);
                }
                Some((
                    Ok::<_, std::convert::Infallible>(event),
                    (stream, task, session),
                ))
            },
        );
        return Ok(axum::response::Sse::new(stream).into_response());
    }
    if suffix == "console/terminal" {
        let endpoint = client.endpoint().to_string();
        let worker = tokio::time::timeout(
            crate::timeouts::Timeouts::default().far_call,
            open_terminal(&endpoint, &target),
        )
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "host_unreachable",
                "worker terminal connection timed out",
            )
        })??;
        let (mut parts, _) = request.into_parts();
        let upgrade = axum::extract::ws::WebSocketUpgrade::from_request_parts(&mut parts, state)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        return Ok(upgrade.on_upgrade(move |mut source| async move {
            let mut worker = worker;
            loop {
                tokio::select! {
                    message = source.recv() => {
                        let Some(Ok(message)) = message else { break };
                        let message = match message {
                            axum::extract::ws::Message::Text(v) => tokio_tungstenite::tungstenite::Message::Text(v.to_string().into()),
                            axum::extract::ws::Message::Binary(v) => tokio_tungstenite::tungstenite::Message::Binary(v),
                            axum::extract::ws::Message::Ping(v) => tokio_tungstenite::tungstenite::Message::Ping(v),
                            axum::extract::ws::Message::Pong(v) => tokio_tungstenite::tungstenite::Message::Pong(v),
                            axum::extract::ws::Message::Close(_) => break,
                        };
                        if worker.send(message).await.is_err() { break; }
                    }
                    message = worker.next() => {
                        let Some(Ok(message)) = message else { break };
                        let message = match message {
                            tokio_tungstenite::tungstenite::Message::Text(v) => axum::extract::ws::Message::Text(v.to_string().into()),
                            tokio_tungstenite::tungstenite::Message::Binary(v) => axum::extract::ws::Message::Binary(v),
                            tokio_tungstenite::tungstenite::Message::Ping(v) => axum::extract::ws::Message::Ping(v),
                            tokio_tungstenite::tungstenite::Message::Pong(v) => axum::extract::ws::Message::Pong(v),
                            tokio_tungstenite::tungstenite::Message::Close(_) => break,
                            _ => continue,
                        };
                        if source.send(message).await.is_err() { break; }
                    }
                }
            }
            let _ = worker.close(None).await;
            let _ = source.close().await;
        }));
    }
    let method = request.method().clone();
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = axum::body::to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?
        .to_vec();
    let response = client
        .forward(method, &target, content_type.as_deref(), bytes)
        .await
        .map_err(|e| super::tasks::far_error(&reference.host, e))?;
    let mut body = response.body;
    if (200..300).contains(&response.status)
        && let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&body)
    {
        rewrite(&mut value, &reference.id, session_id);
        body = serde_json::to_vec(&value).unwrap();
    }
    let mut answer = (StatusCode::from_u16(response.status).unwrap(), body).into_response();
    if let Some(content_type) = response.content_type {
        answer
            .headers_mut()
            .insert("content-type", content_type.parse().unwrap());
    }
    Ok(answer)
}

fn rewrite(value: &mut serde_json::Value, task: &str, session: &str) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if (key == "id" || key == "session_id") && value.as_str() == Some(session) {
                    *value = format!("far~{task}~{session}").into();
                } else if key == "switched_from"
                    && let Some(id) = value.as_str()
                {
                    *value = format!("far~{task}~{id}").into();
                } else {
                    rewrite(value, task, session);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                rewrite(value, task, session);
            }
        }
        _ => {}
    }
}

trait Transport: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Transport for T {}

async fn open_terminal(
    endpoint: &str,
    target: &str,
) -> ApiResult<tokio_tungstenite::WebSocketStream<Box<dyn Transport>>> {
    let failed = |e: String| ApiError::new(StatusCode::BAD_GATEWAY, "host_unreachable", e);
    let (stream, url): (Box<dyn Transport>, String) = if endpoint.starts_with('/') {
        (
            Box::new(
                tokio::net::UnixStream::connect(endpoint)
                    .await
                    .map_err(|e| failed(e.to_string()))?,
            ),
            format!("ws://localhost{target}"),
        )
    } else {
        let parsed = url::Url::parse(endpoint).map_err(|e| failed(e.to_string()))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| failed("worker endpoint has no host".into()))?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| failed("worker endpoint has no port".into()))?;
        (
            Box::new(
                tokio::net::TcpStream::connect((host, port))
                    .await
                    .map_err(|e| failed(e.to_string()))?,
            ),
            format!("{}{target}", endpoint.replacen("http://", "ws://", 1)),
        )
    };
    tokio_tungstenite::client_async(url, stream)
        .await
        .map(|(stream, _)| stream)
        .map_err(|e| failed(e.to_string()))
}
