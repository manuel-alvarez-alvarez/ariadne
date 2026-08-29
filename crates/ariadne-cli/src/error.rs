//! What a failed command prints, and what it exits with: one line, no
//! plumbing, and a code that says what kind of failure it was.
//!
//! Human output gets `error: <sentence>` and nothing else — no anyhow
//! `Caused by:` block, no transport detail, no repetition of the daemon's
//! error envelope. `--format json` gets that envelope instead, so scripts keep
//! the status and code the human line drops.
//!
//! The exit code is the same answer for a script that cannot read either:
//! [`Exit`], documented in `ariadne --help`.

use std::process::ExitCode;

use ariadne_client::ClientError;
use http::StatusCode;

use crate::output::Format;

/// What a failed command exits with. `0` is success and is not in here;
/// everything else says which kind of failure it was, so a script can branch
/// on it without reading prose.
///
/// The list is documented in the root `after_help`; the two must not drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Anything with no better answer.
    Failed = 1,
    /// The command as typed cannot be run: a bad argument, an ambiguous id,
    /// or something irreversible refused for want of `--yes`.
    Usage = 2,
    /// Nothing answered at the endpoint.
    Unreachable = 3,
    /// No goal, task, session, repository or profile of that name.
    NotFound = 4,
    /// The daemon refused: the thing is not in a state that allows it.
    Conflict = 5,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        ExitCode::from(exit as u8)
    }
}

/// A failure the CLI itself decided on, carrying the code it exits with and,
/// where there is one, the way out.
///
/// Everything the daemon decided is a [`ClientError`] and is classified from
/// its status instead ([`exit_code`]).
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Failure {
    exit: Exit,
    message: String,
    hint: Option<String>,
}

impl Failure {
    /// A command that cannot be run as typed: exit [`Exit::Usage`].
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(Exit::Usage, message)
    }

    /// Nothing of that name: exit [`Exit::NotFound`].
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(Exit::NotFound, message)
    }

    /// The thing is not in a state that allows it: exit [`Exit::Conflict`],
    /// the same code the daemon's own 409 gets — a refusal is the refusal
    /// whether it was seen coming or answered by the daemon.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(Exit::Conflict, message)
    }

    fn new(exit: Exit, message: impl Into<String>) -> Self {
        Self {
            exit,
            message: message.into(),
            hint: None,
        }
    }

    /// The one thing to do about it, printed in parentheses after the line —
    /// the same shape a [`ClientError`] hint is printed in.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// This failure as the error the commands pass around.
    pub fn err(self) -> anyhow::Error {
        anyhow::Error::new(self)
    }

    /// The slug `--format json` reports it under.
    fn code(&self) -> &'static str {
        match self.exit {
            Exit::Failed => "cli_error",
            Exit::Usage => "usage",
            Exit::Unreachable => "daemon_unreachable",
            Exit::NotFound => "not_found",
            Exit::Conflict => "conflict",
        }
    }
}

/// Print a failed command's error and nothing more.
pub fn report(err: &anyhow::Error, format: Format) {
    match format {
        Format::Json => eprintln!("{}", serde_json::json!({"error": json_error(err)})),
        Format::Table => eprintln!("error: {}", human_line(err)),
    }
}

/// What the process exits with: what the CLI decided, else what the daemon's
/// answer amounts to, else "something went wrong".
pub fn exit_code(err: &anyhow::Error) -> ExitCode {
    exit(err).into()
}

/// The same, as the enum — what the mapping is actually tested on.
pub fn exit(err: &anyhow::Error) -> Exit {
    if let Some(failure) = err.chain().find_map(|e| e.downcast_ref::<Failure>()) {
        return failure.exit;
    }
    match client_error(err) {
        // A daemon that never answered and one that took too long are the
        // same fact to whoever is waiting on it.
        Some(ClientError::Unreachable { .. } | ClientError::Timeout) => Exit::Unreachable,
        Some(ClientError::Api { status, .. }) => match *status {
            StatusCode::NOT_FOUND => Exit::NotFound,
            StatusCode::CONFLICT => Exit::Conflict,
            _ => Exit::Failed,
        },
        _ => Exit::Failed,
    }
}

/// The one line a human reads.
pub fn human_line(err: &anyhow::Error) -> String {
    // A daemon-side failure already reads as prose, and it is the whole story:
    // the transport source and the envelope's machine half stay out of it.
    if let Some(client) = client_error(err) {
        return match client.hint() {
            Some(hint) => format!("{} ({hint})", client.human()),
            None => client.human(),
        };
    }
    if let Some(failure) = err.chain().find_map(|e| e.downcast_ref::<Failure>())
        && let Some(hint) = &failure.hint
    {
        return format!("{} ({hint})", flatten(err));
    }
    flatten(err)
}

/// The error as the API-shaped envelope: `code` and `message` as the daemon
/// sent them, plus the status it answered with.
fn json_error(err: &anyhow::Error) -> serde_json::Value {
    let Some(client) = client_error(err) else {
        return match err.chain().find_map(|e| e.downcast_ref::<Failure>()) {
            Some(failure) => {
                let mut out = serde_json::json!({"code": failure.code(), "message": flatten(err)});
                if let Some(hint) = &failure.hint {
                    out.as_object_mut()
                        .expect("json object")
                        .insert("hint".into(), hint.as_str().into());
                }
                out
            }
            None => serde_json::json!({"code": "cli_error", "message": flatten(err)}),
        };
    };
    let mut out = serde_json::json!({"code": client.code(), "message": client.human()});
    let map = out.as_object_mut().expect("json object");
    if let ClientError::Api { status, .. } = client {
        map.insert("status".into(), status.as_u16().into());
    }
    if let Some(hint) = client.hint() {
        map.insert("hint".into(), hint.into());
    }
    out
}

/// The daemon-side failure behind an error, if that is what went wrong.
fn client_error(err: &anyhow::Error) -> Option<&ClientError> {
    err.chain().find_map(|e| e.downcast_ref::<ClientError>())
}

/// An anyhow context chain on one line: `outer: inner`, skipping links that
/// only repeat what the link above already quoted.
fn flatten(err: &anyhow::Error) -> String {
    let mut line = String::new();
    for cause in err.chain() {
        let part = cause.to_string();
        if line.is_empty() {
            line = part;
        } else if !line.contains(&part) {
            line = format!("{line}: {part}");
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unreachable() -> ClientError {
        ClientError::Unreachable {
            endpoint: "/tmp/x.sock".into(),
            source: "client error (Connect)".into(),
        }
    }

    fn not_found() -> ClientError {
        ClientError::Api {
            status: StatusCode::NOT_FOUND,
            code: "not_found".into(),
            message: "task not found: badid123".into(),
        }
    }

    /// Used to be `cannot reach ... : client error (Connect)` plus a `Caused
    /// by:` chain repeating it.
    #[test]
    fn an_unreachable_daemon_is_one_line_with_the_way_out() {
        assert_eq!(
            human_line(&anyhow::Error::new(unreachable())),
            "cannot reach the ariadne daemon at /tmp/x.sock \
             (is it running? try: ariadne daemon start)"
        );
    }

    /// Used to be `daemon returned 404 Not Found: not_found: task not found`.
    #[test]
    fn an_api_error_is_just_the_daemons_message() {
        assert_eq!(
            human_line(&anyhow::Error::new(not_found())),
            "task not found: badid123"
        );
    }

    /// The human line drops status and code; `--format json` keeps them.
    #[test]
    fn json_output_keeps_the_machine_readable_half() {
        assert_eq!(
            json_error(&anyhow::Error::new(not_found())),
            serde_json::json!({
                "code": "not_found",
                "message": "task not found: badid123",
                "status": 404,
            })
        );
        assert_eq!(
            json_error(&anyhow::Error::new(unreachable())),
            serde_json::json!({
                "code": "daemon_unreachable",
                "message": "cannot reach the ariadne daemon at /tmp/x.sock",
                "hint": "is it running? try: ariadne daemon start",
            })
        );
    }

    /// Context wrapped around a daemon failure never gets to say it twice.
    #[test]
    fn a_wrapped_client_error_still_speaks_for_itself() {
        let err = anyhow::Error::new(not_found()).context("inspecting task badid123");
        assert_eq!(human_line(&err), "task not found: badid123");
    }

    /// A local failure keeps its context, flattened onto the same line.
    #[test]
    fn a_local_failure_reads_as_context_then_cause() {
        let err = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory");
        let err = anyhow::Error::new(err).context("reading /tmp/prompt.md");
        assert_eq!(
            human_line(&err),
            "reading /tmp/prompt.md: No such file or directory"
        );
    }

    /// A context that already quotes its source says it once.
    #[test]
    fn a_repeated_cause_is_not_printed_twice() {
        let err = anyhow::anyhow!("boom").context("failed: boom");
        assert_eq!(human_line(&err), "failed: boom");
    }

    #[test]
    fn a_bare_message_is_the_whole_line() {
        assert_eq!(
            human_line(&anyhow::anyhow!("provide --prompt or --prompt-file")),
            "provide --prompt or --prompt-file"
        );
    }

    /// The whole point of the codes: a script tells a missing id from a
    /// daemon that is not there from a refusal, without reading the line.
    #[test]
    fn every_kind_of_failure_has_an_exit_code_of_its_own() {
        let api = |status: StatusCode| {
            anyhow::Error::new(ClientError::Api {
                status,
                code: "whatever".into(),
                message: "no".into(),
            })
        };
        assert_eq!(exit(&anyhow::Error::new(unreachable())), Exit::Unreachable);
        assert_eq!(
            exit(&anyhow::Error::new(ClientError::Timeout)),
            Exit::Unreachable,
            "a daemon that never finished answering is one that did not answer"
        );
        assert_eq!(exit(&api(StatusCode::NOT_FOUND)), Exit::NotFound);
        assert_eq!(exit(&api(StatusCode::CONFLICT)), Exit::Conflict);
        assert_eq!(exit(&api(StatusCode::BAD_REQUEST)), Exit::Failed);
        assert_eq!(exit(&anyhow::anyhow!("boom")), Exit::Failed);
        assert_eq!(exit(&Failure::usage("refusing").err()), Exit::Usage);
        assert_eq!(exit(&Failure::not_found("no task").err()), Exit::NotFound);
        assert_eq!(
            exit(&Failure::conflict("goal is planning").err()),
            Exit::Conflict,
            "a refusal we saw coming exits as the daemon's own would have"
        );
    }

    /// A code the CLI decided on survives the context wrapped around it: the
    /// command that failed says what it was doing, not what it exits with.
    #[test]
    fn context_does_not_change_what_a_failure_exits_with() {
        let err = Failure::not_found("no task matches \"01x\"")
            .hint("ariadne task ls lists them")
            .err()
            .context("inspecting task 01x");
        assert_eq!(exit(&err), Exit::NotFound);
        assert_eq!(
            human_line(&err),
            "inspecting task 01x: no task matches \"01x\" (ariadne task ls lists them)"
        );
    }

    /// The same failure as a script reads it: the slug of its code, its line,
    /// and the way out beside it.
    #[test]
    fn a_cli_failure_carries_its_code_and_hint_into_json() {
        let err = Failure::not_found("no task matches \"01x\"")
            .hint("ariadne task ls lists them")
            .err();
        assert_eq!(
            json_error(&err),
            serde_json::json!({
                "code": "not_found",
                "message": "no task matches \"01x\"",
                "hint": "ariadne task ls lists them",
            })
        );
    }

    /// A non-daemon failure is still reportable as json.
    #[test]
    fn json_output_falls_back_to_a_cli_error() {
        let err = anyhow::anyhow!("no daemon log at /tmp/h/ariadned.log");
        assert_eq!(
            json_error(&err),
            serde_json::json!({
                "code": "cli_error",
                "message": "no daemon log at /tmp/h/ariadned.log",
            })
        );
    }

    #[test]
    fn every_layer_of_context_survives() {
        let err = anyhow::anyhow!("permission denied")
            .context("opening log file in /tmp/h")
            .context("starting ariadned");
        assert_eq!(
            human_line(&err),
            "starting ariadned: opening log file in /tmp/h: permission denied"
        );
    }
}
