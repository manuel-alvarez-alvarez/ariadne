//! Integration tests for `POST /v1/sessions/{id}/resize`.
//!
//! tmux is stubbed by a script that records its argv, so the assertions are on
//! the exact invocation the daemon makes. That is the contract here: a
//! detached pane only honours a size when sizing has been taken off tmux's
//! hands, and only gives it back to a client that attaches later if the hook
//! that does so went out with it. The geometry a real tmux ends up at is
//! checked against a real tmux in `managers.rs`.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};

use ariadne_api::error::ErrorBody;
use ariadne_core::SessionStatus;

use common::{Harness, harness, post_json};

fn post_resize(session_id: &str, body: serde_json::Value) -> Request<Body> {
    post_json(&format!("/v1/sessions/{session_id}/resize"), body)
}

fn size(cols: u16, rows: u16) -> serde_json::Value {
    serde_json::json!({ "cols": cols, "rows": rows })
}

/// The sizing calls only — everything but the liveness probes.
fn sizing_calls(h: &Harness) -> Vec<String> {
    h.tmux_calls()
        .into_iter()
        .filter(|call| !call.starts_with("has-session"))
        .collect()
}

#[tokio::test]
async fn a_resize_sizes_the_window_and_leaves_a_client_free_to_resize_it_again() {
    let h = harness().await;
    let session = h.lone_session("ariadne-resize").await;
    h.every_pane_exists();

    let (status, _) = h.send(post_resize(&session.id, size(137, 41))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        sizing_calls(&h),
        vec![
            "set-hook -t ariadne-resize client-attached set-window-option -u window-size ; \
             set-window-option -t ariadne-resize window-size manual ; \
             resize-window -t ariadne-resize -x 137 -y 41"
                .to_string()
        ],
        "one invocation: the hook that gives sizing back to a client, the manual \
         sizing a detached pane needs, and the size itself"
    );
}

#[tokio::test]
async fn a_finished_session_refuses_a_resize() {
    let h = harness().await;
    let session = h.lone_session("ariadne-finished").await;
    // Its pane is alive — a successor took the name over — so only the stored
    // status says this session is done with.
    h.every_pane_exists();
    h.set_status(&session, SessionStatus::Exited).await;

    let envelope: ErrorBody = h
        .error(post_resize(&session.id, size(120, 40)), StatusCode::CONFLICT)
        .await;
    assert_eq!(envelope.error.code, "conflict");
    assert!(
        sizing_calls(&h).is_empty(),
        "the pane of a session that is over is left alone"
    );
}

/// The status still says live but tmux is gone: the pane to resize does not
/// exist, and a stale name may belong to a successor's pane by now.
#[tokio::test]
async fn a_session_without_a_pane_refuses_a_resize() {
    let h = harness().await;
    let session = h.lone_session("ariadne-no-pane").await;

    let (status, _) = h.send(post_resize(&session.id, size(120, 40))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(sizing_calls(&h).is_empty());
}

/// A viewer that measured its panel wrong must not reach tmux with the answer:
/// a zero side is not a grid, and a pane is a real allocation per cell.
#[tokio::test]
async fn a_size_outside_the_bounds_is_rejected_before_tmux_sees_it() {
    let h = harness().await;
    let session = h.lone_session("ariadne-bounds").await;
    h.every_pane_exists();

    for out_of_range in [size(0, 24), size(80, 0), size(501, 24), size(80, 501)] {
        let envelope: ErrorBody = h
            .error(
                post_resize(&session.id, out_of_range.clone()),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert_eq!(
            envelope.error.code, "invalid_request",
            "{out_of_range} is not a pane size"
        );
    }
    // The largest grid that is still a grid is one, and so is the cap itself.
    for accepted in [size(1, 1), size(500, 500)] {
        let (status, _) = h.send(post_resize(&session.id, accepted.clone())).await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "{accepted} is within the bounds"
        );
    }

    assert_eq!(
        sizing_calls(&h).len(),
        2,
        "only the two accepted sizes reached tmux: {:?}",
        sizing_calls(&h)
    );
}
