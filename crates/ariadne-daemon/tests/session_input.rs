//! Integration tests for `POST /v1/sessions/{id}/input`.
//!
//! tmux is stubbed by a script that records its argv, so the assertions are on
//! the exact `send-keys` invocation the daemon makes — which is the whole
//! contract: control bytes and escape sequences have to reach the pane
//! unchanged, and nothing may be appended to them.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};

use ariadne_core::SessionStatus;

use common::{Harness, harness, post_json};

use ariadne_api::error::ErrorBody;

fn post_input(session_id: &str, data: &str) -> Request<Body> {
    post_json(
        &format!("/v1/sessions/{session_id}/input"),
        serde_json::json!({ "data": data }),
    )
}

/// Hex-encoded argument list for `send-keys -H`, as the stub records it.
fn hex(data: &str) -> String {
    data.bytes()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn send_keys(h: &Harness) -> Vec<String> {
    h.tmux_calls_of("send-keys")
}

#[tokio::test]
async fn typing_reaches_the_pane_byte_for_byte() {
    let h = harness().await;
    let session = h.lone_session("ariadne-typing").await;
    h.every_pane_exists();

    // What a terminal in front of a user actually emits: printable text, a
    // Return, a Ctrl-C, an Up-arrow escape sequence, and a multi-byte glyph.
    let typed = "ls -la\r\u{3}\u{1b}[A│";
    let (status, _) = h.send(post_input(&session.id, typed)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        send_keys(&h),
        vec![format!("send-keys -t ariadne-typing -H {}", hex(typed))],
        "the input goes out as one send-keys, hex-encoded, with nothing appended"
    );
    // `send_submitted` presses Enter of its own; this endpoint must not — the
    // terminal already sent its own `\r` when the user pressed Return.
    assert!(
        !send_keys(&h)[0].contains("Enter"),
        "no key-name Enter is sent"
    );
}

#[tokio::test]
async fn a_finished_session_refuses_input() {
    let h = harness().await;
    let session = h.lone_session("ariadne-finished").await;
    // Its pane is alive — a successor took the name over — so only the stored
    // status says this session is done with.
    h.every_pane_exists();
    h.set_status(&session, SessionStatus::Exited).await;

    let envelope: ErrorBody = h
        .error(post_input(&session.id, "rm -rf /\r"), StatusCode::CONFLICT)
        .await;
    assert_eq!(envelope.error.code, "conflict");
    assert!(
        send_keys(&h).is_empty(),
        "nothing is typed into the pane of a session that is over"
    );
}

/// The status still says live but tmux is gone: the pane the daemon would
/// type into does not exist, so the request fails rather than silently doing
/// nothing.
#[tokio::test]
async fn a_session_without_a_pane_refuses_input() {
    let h = harness().await;
    let session = h.lone_session("ariadne-no-pane").await;

    let (status, _) = h.send(post_input(&session.id, "hello")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(send_keys(&h).is_empty());
}

#[tokio::test]
async fn an_unknown_session_yields_the_standard_error_envelope() {
    let h = harness().await;

    let envelope: ErrorBody = h
        .error(
            post_input("01ARZ3NDEKTSV4RRFFQ69G5FAV", "hello"),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert_eq!(envelope.error.code, "not_found");
}

/// A paste is longer than one argv can comfortably hold, so it is split — but
/// only into consecutive batches, in order, with no byte lost or duplicated.
#[tokio::test]
async fn a_long_paste_is_split_into_ordered_batches() {
    let h = harness().await;
    let session = h.lone_session("ariadne-paste").await;
    h.every_pane_exists();

    let pasted: String = (0..1500)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    let (status, _) = h.send(post_input(&session.id, &pasted)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let sent: String = send_keys(&h)
        .iter()
        .flat_map(|call| {
            let hexes = call.split(" -H ").nth(1).expect("a -H payload").to_string();
            hexes
                .split_whitespace()
                .map(|byte| u8::from_str_radix(byte, 16).unwrap() as char)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(sent, pasted, "the batches reassemble into what was pasted");
    assert!(send_keys(&h).len() > 1, "1500 bytes do not fit one batch");
}
