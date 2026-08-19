//! Query-string building over the shared `ariadne-api` query types.

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

#[cfg(test)]
mod tests {
    use super::query_path;

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
}
