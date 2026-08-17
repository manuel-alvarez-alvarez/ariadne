//! What a failed command prints: one line, no plumbing.
//!
//! Human output gets `error: <sentence>` and nothing else — no anyhow
//! `Caused by:` block, no transport detail, no repetition of the daemon's
//! error envelope. `--format json` gets that envelope instead, so scripts keep
//! the status and code the human line drops.

use ariadne_client::ClientError;

use crate::output::Format;

/// Print a failed command's error and nothing more. Exit code stays 1 (usage
/// errors exit 2, from clap, before we ever get here).
pub fn report(err: &anyhow::Error, format: Format) {
    match format {
        Format::Json => eprintln!("{}", serde_json::json!({"error": json_error(err)})),
        Format::Table => eprintln!("error: {}", human_line(err)),
    }
}

/// The one line a human reads.
fn human_line(err: &anyhow::Error) -> String {
    // A daemon-side failure already reads as prose, and it is the whole story:
    // the transport source and the envelope's machine half stay out of it.
    if let Some(client) = client_error(err) {
        return match client.hint() {
            Some(hint) => format!("{} ({hint})", client.human()),
            None => client.human(),
        };
    }
    flatten(err)
}

/// The error as the API-shaped envelope: `code` and `message` as the daemon
/// sent them, plus the status it answered with.
fn json_error(err: &anyhow::Error) -> serde_json::Value {
    let Some(client) = client_error(err) else {
        return serde_json::json!({"code": "cli_error", "message": flatten(err)});
    };
    let mut out = serde_json::json!({"code": client.code(), "message": client.human()});
    let map = out.as_object_mut().expect("json object");
    if let ClientError::Api {
        status, details, ..
    } = client
    {
        map.insert("status".into(), status.as_u16().into());
        if let Some(details) = details {
            map.insert("details".into(), details.clone());
        }
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

    use http::StatusCode;

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
            details: None,
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
