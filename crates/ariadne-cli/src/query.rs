//! Path and query-string building over the shared `ariadne-api` query types.

use anyhow::Result;
use serde::Serialize;

/// Append `query` to `base` as a URL-encoded query string. Filters that are
/// `None` are omitted; when nothing remains, `base` is returned untouched
/// (no stray `?`).
pub fn query_path(base: &str, query: &impl Serialize) -> Result<String> {
    let qs = serde_urlencoded::to_string(query)?;
    Ok(if qs.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{qs}")
    })
}

/// The hex digits a percent-escape is spelled with.
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// One caller-typed value as a single path segment.
///
/// Profiles answer to their name as well as their id, and a name is free text
/// — a profile named `My Integrator` has a space in it, and a space is not a
/// character a URI may carry: `ariadne profile inspect "My Integrator"` used
/// to reach the client with it raw and panic on the URI it could not build.
/// Everything outside the unreserved set (RFC 3986 §2.3) is escaped rather
/// than only what is known to hurt, and `/` with it: the value is one
/// whole segment, so a slash inside it is data, never structure.
pub fn path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0xf) as usize] as char);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{path_segment, query_path};

    use ariadne_api::sessions::SessionListQuery;
    use ariadne_api::tasks::TaskListQuery;
    use ariadne_core::{SessionStatus, TaskStatus};

    #[test]
    fn task_list_query_empty_has_no_separators() {
        let path = query_path("/v1/tasks", &TaskListQuery::default()).unwrap();
        assert_eq!(path, "/v1/tasks");
    }

    #[test]
    fn task_list_query_goal_only() {
        let query = TaskListQuery {
            goal: Some("01ABC".into()),
            status: None,
        };
        assert_eq!(
            query_path("/v1/tasks", &query).unwrap(),
            "/v1/tasks?goal=01ABC"
        );
    }

    #[test]
    fn task_list_query_status_uses_wire_spelling() {
        let query = TaskListQuery {
            goal: None,
            status: Some(TaskStatus::UnderReview),
        };
        assert_eq!(
            query_path("/v1/tasks", &query).unwrap(),
            "/v1/tasks?status=under_review"
        );
    }

    #[test]
    fn task_list_query_both_filters() {
        let query = TaskListQuery {
            goal: Some("01ABC".into()),
            status: Some(TaskStatus::Merged),
        };
        assert_eq!(
            query_path("/v1/tasks", &query).unwrap(),
            "/v1/tasks?goal=01ABC&status=merged"
        );
    }

    #[test]
    fn values_are_url_encoded() {
        let query = TaskListQuery {
            goal: Some("a b&c".into()),
            status: None,
        };
        assert_eq!(
            query_path("/v1/tasks", &query).unwrap(),
            "/v1/tasks?goal=a+b%26c"
        );
    }

    #[test]
    fn session_list_query_empty_has_no_separators() {
        let path = query_path("/v1/sessions", &SessionListQuery::default()).unwrap();
        assert_eq!(path, "/v1/sessions");
    }

    /// `session ls --status failed` is answered by the daemon, not filtered
    /// once the whole list is here.
    #[test]
    fn session_list_query_status_uses_wire_spelling() {
        let query = SessionListQuery {
            status: Some(SessionStatus::Failed),
            ..SessionListQuery::default()
        };
        assert_eq!(
            query_path("/v1/sessions", &query).unwrap(),
            "/v1/sessions?status=failed"
        );
    }

    /// The ids every other command passes through here are untouched.
    #[test]
    fn an_id_comes_out_as_it_went_in() {
        assert_eq!(
            path_segment("01M0R9EPJK7QYAGYCN31E8EF58"),
            "01M0R9EPJK7QYAGYCN31E8EF58"
        );
    }

    /// The one that made this necessary: a profile with a space in its name,
    /// which is how a user names it on the command line.
    #[test]
    fn a_name_with_a_space_is_escaped_rather_than_sent_raw() {
        assert_eq!(path_segment("My Integrator"), "My%20Integrator");
    }

    /// A whole segment, so nothing in it may be read as structure.
    #[test]
    fn a_name_cannot_smuggle_in_another_path_segment() {
        assert_eq!(path_segment("../goals/01G"), "..%2Fgoals%2F01G");
        assert_eq!(path_segment("a?b#c"), "a%3Fb%23c");
    }

    /// Names are stored as text, not as bytes: a non-ASCII one round-trips
    /// through UTF-8 rather than being dropped.
    #[test]
    fn a_non_ascii_name_is_escaped_by_its_utf8_bytes() {
        assert_eq!(path_segment("Revisor Estrícto"), "Revisor%20Estr%C3%ADcto");
    }
}
