//! Following: what every `-f` and `--watch` in the CLI is made of.
//!
//! Two shapes, over the daemon's server-sent streams. A *tail* prints frames
//! as they arrive ([`frames`], and [`frames_reconnecting`] for one that is
//! meant to outlive a daemon restart); a *watch* uses the frames only as a
//! signal to draw the whole thing again ([`watch`]), the way `watch(1)` does,
//! because a table is a picture of the state and not a log of it.
//!
//! Both end on Ctrl-C, and end by *returning*: the terminal is put back the
//! way it was found on the way out, which a signal that killed the process
//! would not have done. That is why the interrupt is awaited as a future here
//! rather than left to the default handler — and why *every* wait a follow
//! mode does races it, the opening request included.

use std::future::Future;
use std::io::{IsTerminal, Write};
use std::time::Duration;

use anyhow::Result;
use tokio::time::{Instant, sleep, sleep_until};

use ariadne_client::{Client, ClientError, SseEvent, SseStream};

use crate::error::client_error;
use crate::output::note;

/// How long a redraw waits for the burst it is in to finish.
///
/// One thing happening produces several events — a task transition is a
/// `task_updated` and the `review_created` and `session_updated` around it —
/// and redrawing on each of them is a flicker showing three views of one
/// moment. Short enough that a watch still feels immediate.
const SETTLE: Duration = Duration::from_millis(250);

/// How long a dropped follow waits before dialling again, and the longest it
/// waits once the daemon has been unanswered for a while.
///
/// The wait doubles between tries so that a daemon that is coming right back
/// is picked up in a beat, and one that is being reinstalled is not dialled a
/// thousand times while it is.
const RETRY: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(15);

/// Whether a frame handler wants the next frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Keep reading.
    Go,
    /// This was the last frame worth having — the session ended, say.
    Stop,
}

/// How a follow ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The handler said [`Next::Stop`].
    Done,
    /// Ctrl-C.
    Interrupted,
    /// The daemon closed the connection, or the connection broke.
    Dropped,
}

/// Follow one connection to `path`, handing every frame to `on_frame`.
///
/// Returns as soon as the handler stops, the daemon hangs up or the user
/// interrupts — the caller decides which of those deserves a word.
pub async fn frames(
    client: &Client,
    path: &str,
    mut on_frame: impl FnMut(SseEvent) -> Result<Next>,
) -> Result<Ending> {
    match open(client, path).await? {
        Some(mut stream) => read(&mut stream, &mut on_frame).await,
        None => Ok(Ending::Interrupted),
    }
}

/// The same, redialled whenever the daemon drops the connection: a follow that
/// is meant to run until the user stops it.
///
/// The daemon restarting, or hanging up on a follower that fell behind, is not
/// the end of a `-f`; it is said on stderr and the stream is picked up again.
/// Only the *first* connection can fail outright, which is how a caller with
/// somewhere else to look — `daemon logs` and its file — hears that the daemon
/// was never there.
pub async fn frames_reconnecting(
    client: &Client,
    path: &str,
    mut on_frame: impl FnMut(SseEvent) -> Result<Next>,
) -> Result<()> {
    let Some(mut stream) = open(client, path).await? else {
        return Ok(());
    };
    loop {
        match read(&mut stream, &mut on_frame).await? {
            Ending::Interrupted | Ending::Done => return Ok(()),
            Ending::Dropped => note("the daemon closed the stream — reconnecting"),
        }
        match reconnect(client, path).await {
            Some(fresh) => stream = fresh,
            None => return Ok(()),
        }
    }
}

/// Open a stream, giving the user the interrupt while it is being opened.
///
/// `None` when they took it. The open is a wait like any other and a long one:
/// a daemon that accepts the connection and then sends nothing holds it for
/// the whole timeout the client gives a response, and Ctrl-C would have no
/// effect for all of it — the CLI has taken the signal over, so a wait that
/// does not race it is a wait nothing can end.
async fn open(client: &Client, path: &str) -> Result<Option<SseStream>> {
    match unless_interrupted(client.stream(path)).await {
        Some(opened) => Ok(Some(opened?)),
        None => Ok(None),
    }
}

/// Hand every frame of one open stream to `on_frame`, until it stops, the
/// stream ends or the user interrupts.
async fn read(
    stream: &mut SseStream,
    on_frame: &mut impl FnMut(SseEvent) -> Result<Next>,
) -> Result<Ending> {
    loop {
        tokio::select! {
            () = interrupt() => return Ok(Ending::Interrupted),
            frame = stream.next() => match frame {
                Some(Ok(event)) => {
                    if on_frame(event)? == Next::Stop {
                        return Ok(Ending::Done);
                    }
                }
                // A broken connection is the same thing as a closed one to a
                // follow: there is nothing more coming down this one.
                Some(Err(_)) | None => return Ok(Ending::Dropped),
            },
        }
    }
}

/// Open `path` again, waiting longer after each try the daemon does not
/// answer. `None` when the user interrupted the waiting.
///
/// It never gives up on its own. A daemon being restarted is exactly when a
/// follow is worth keeping, and one that quit after so many tries would be a
/// watch that silently stopped watching — the failure a person would only
/// notice by the screen having gone quiet. Ctrl-C is what ends it, and every
/// wait here is interruptible.
async fn reconnect(client: &Client, path: &str) -> Option<SseStream> {
    let mut wait = RETRY;
    loop {
        if interrupted(wait).await {
            return None;
        }
        match unless_interrupted(client.stream(path)).await? {
            Ok(stream) => return Some(stream),
            Err(e) => {
                note(&format!("{} — reconnecting", e.human()));
                wait = (wait * 2).min(RETRY_MAX);
            }
        }
    }
}

/// Wait `pause` out, or report that the user interrupted it first.
async fn interrupted(pause: Duration) -> bool {
    unless_interrupted(sleep(pause)).await.is_none()
}

/// What `work` answered with, or `None` if the user interrupted the waiting.
async fn unless_interrupted<T>(work: impl Future<Output = T>) -> Option<T> {
    first(interrupt(), work).await
}

/// Whichever of the two comes first: the work's value, or `None` for the
/// interruption.
///
/// Split out from [`unless_interrupted`] so that the race can be tested for
/// what it is — a test cannot raise the signal at itself without every other
/// test in the binary hearing it.
async fn first<T>(stop: impl Future<Output = ()>, work: impl Future<Output = T>) -> Option<T> {
    tokio::select! {
        () = stop => None,
        value = work => Some(value),
    }
}

/// Ctrl-C, as a plain future.
///
/// A handler that could not be installed resolves as *never* rather than as an
/// interruption: tokio reports that failure by resolving, and taking it for a
/// Ctrl-C would end every follow mode the instant it began.
async fn interrupt() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await
    }
}

/// Draw `render`, then draw it again whenever a frame `relevant` cares about
/// arrives — `watch(1)` over the daemon's event stream, until Ctrl-C.
///
/// The stream is only ever a signal: what is shown is whatever `render` reads
/// afresh, so a redraw is the current state and not a state patched up from
/// events. The connection is held across redraws rather than reopened after
/// each one, which is what keeps a change made while the screen was being
/// drawn from being missed.
pub async fn watch(
    client: &Client,
    path: &str,
    relevant: impl Fn(&SseEvent) -> bool,
    render: impl AsyncFn() -> Result<()>,
) -> Result<()> {
    let _cursor = Cursor::hidden();
    let Some(opened) = open(client, path).await? else {
        return Ok(());
    };
    let mut stream = Some(opened);
    loop {
        clear_screen();
        render().await?;
        match &mut stream {
            Some(live) => match settled(live, &relevant).await {
                Waited::Interrupted => return Ok(()),
                Waited::Changed => {}
                Waited::Dropped => stream = None,
            },
            // The daemon hung up. Dial again, and the redraw at the top of the
            // loop is what catches up on everything that changed while it was
            // away — none of which this connection was told about.
            None => match reconnect(client, path).await {
                Some(live) => stream = Some(live),
                None => return Ok(()),
            },
        }
    }
}

/// What ended one wait between redraws.
enum Waited {
    /// Something the watch cares about changed, and the burst it was part of
    /// has settled: draw again.
    Changed,
    /// Ctrl-C.
    Interrupted,
    /// The daemon closed the connection.
    Dropped,
}

/// Wait for a relevant frame and then for its burst to settle, so one thing
/// happening is one redraw.
async fn settled(stream: &mut SseStream, relevant: &impl Fn(&SseEvent) -> bool) -> Waited {
    let mut redraw_at: Option<Instant> = None;
    loop {
        tokio::select! {
            () = interrupt() => return Waited::Interrupted,
            // Pending until something has actually changed: with no deadline
            // armed this branch never fires, and the wait is the stream's.
            () = async { match redraw_at {
                Some(at) => sleep_until(at).await,
                None => std::future::pending().await,
            } } => return Waited::Changed,
            frame = stream.next() => match frame {
                Some(Ok(event)) if relevant(&event) => {
                    redraw_at = Some(Instant::now() + SETTLE);
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return Waited::Dropped,
            },
        }
    }
}

/// Whether what we print is being watched by eyes: escape sequences are for a
/// terminal, and a redirected `--watch` should write plain output.
fn to_a_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// Erase the screen and put the cursor back at the top, the way `watch(1)`
/// starts each frame. A pipe gets the frames one after another instead.
fn clear_screen() {
    if to_a_terminal() {
        print!("\x1b[2J\x1b[H");
        let _ = std::io::stdout().flush();
    }
}

/// The hidden cursor of a redrawing screen, and the promise to give it back.
///
/// A watch that left the cursor hidden would leave the shell it returns to
/// looking broken, so restoring it is a `Drop`: it runs whether the watch
/// ended on Ctrl-C, on an error or on a daemon that never came back.
struct Cursor(bool);

impl Cursor {
    fn hidden() -> Self {
        let terminal = to_a_terminal();
        if terminal {
            print!("\x1b[?25l");
            let _ = std::io::stdout().flush();
        }
        Self(terminal)
    }
}

impl Drop for Cursor {
    fn drop(&mut self) {
        if self.0 {
            print!("\x1b[?25h");
            let _ = std::io::stdout().flush();
        }
    }
}

/// Whether a failure means the daemon is not there — the one failure
/// `daemon logs` answers with the log file rather than with an error.
pub fn unreachable(e: &anyhow::Error) -> bool {
    matches!(client_error(e), Some(ClientError::Unreachable { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::future::{pending, ready};

    /// The race itself: whichever finishes first decides, and an interruption
    /// is the answer even when the work would never have finished.
    #[tokio::test]
    async fn a_wait_that_never_ends_is_ended_by_the_interruption() {
        assert_eq!(first(ready(()), pending::<u8>()).await, None);
        assert_eq!(first(pending(), ready(7u8)).await, Some(7));
    }

    /// The failure this race exists for: a daemon that accepts the connection
    /// and then never sends a header. The opening request used to be awaited
    /// on its own, so Ctrl-C did nothing for the whole 30 s the client gives a
    /// response — the one wait in a follow mode with no way out of it.
    #[tokio::test]
    async fn an_open_whose_headers_never_come_is_still_interruptible() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("silent.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("bind");
        // Accepted and held: no status line, no headers, ever. The connections
        // are kept alive by the task owning them.
        let _accepting = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((connection, _)) = listener.accept().await {
                held.push(connection);
            }
        });

        let client = Client::unix(&socket);
        let began = std::time::Instant::now();
        let opened = first(
            async { sleep(Duration::from_millis(50)).await },
            client.stream("/v1/events/stream"),
        )
        .await;

        assert!(opened.is_none(), "the interruption won the race");
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "and won it at once, not after the client's response timeout: {:?}",
            began.elapsed()
        );
    }
}
