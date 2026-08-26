//! Live session-log streaming: the push counterpart of the `/logs` snapshot.
//!
//! The contract is here; how a pane is read without ever showing a client a
//! screen at the wrong grid is in [`super::pane`].

use std::convert::Infallible;

use axum::extract::{Path, State};
use axum::response::sse::{Event, Sse};
use futures_util::Stream;

use ariadne_api::sessions::SessionLogChunk;

use super::AppState;
use super::error::ApiResult;
use super::{pane, sse};

/// Follow a session's terminal output.
///
/// The stream opens with a `resize` event (`SessionPaneSize`) carrying the
/// grid the output is drawn at: the snapshot is wrapped at that width and
/// every later repaint is addressed in it. A live pane is measured; a
/// finished one is reported at the last size it was seen at, if it ever was.
///
/// Then a `snapshot` event carrying the scrollback the `/logs` endpoint would
/// return — as the pane's screen rather than as text: it ends where the pane's
/// cursor is (see [`as_screen`]), so the repaints that follow land where they
/// were addressed. Then a `delta` event per burst of new output. Both payloads
/// are a `SessionLogChunk`: raw terminal bytes, escape sequences and all, are
/// JSON-encoded so they cannot break SSE's line framing.
///
/// A pane resized under the stream — by `ariadne attach`, say — sends a
/// `resize` and a *fresh* `snapshot` rather than continuing with deltas: the
/// output in flight straddles the change and belongs to neither grid, so the
/// client starts over at the new one. `snapshot` therefore means "replace
/// everything you have", whenever it arrives. Nothing is sent in between: a
/// delta drawn at a grid the client does not have is the corruption this is
/// all here to avoid. If no coherent screen can be had — the pane cannot be
/// read, or keeps changing shape while it is — the connection is closed
/// *without* an `end`, at the opening as much as later on: the session is not
/// over, and a fresh connection is the shortest way back to a grid and a
/// screen that agree. Only a pane confirmed gone ends a stream.
///
/// When the session ends — or if it was already over when the request arrived
/// — the remaining output is flushed, a final `end` event (`SessionLogEnd`)
/// is sent and the connection closes. There is no replay and no
/// `Last-Event-ID`: reconnecting starts again from a fresh snapshot.
#[utoipa::path(get, path = "/v1/sessions/{id}/logs/stream", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200,
        description = "SSE stream of terminal output (text/event-stream). A `resize` event \
                       with the grid the output is drawn at (`{\"cols\": 80, \"rows\": 24}`, \
                       SessionPaneSize), then a `snapshot` event with the current scrollback \
                       and a `delta` event per burst of new output — both \
                       `{\"chunk\": \"...\"}` (SessionLogChunk). A pane resized under the \
                       stream sends a new `resize` followed by a fresh `snapshot`, which \
                       replaces everything sent so far. A final `end` event (SessionLogEnd) \
                       closes the stream when the session is over.",
        content_type = "text/event-stream", body = SessionLogChunk),
        (status = 404)))]
pub async fn logs_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let session = state.store.get_session(&id).await?;
    Ok(sse::respond(pane::follow(state, session)))
}
