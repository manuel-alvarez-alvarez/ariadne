//! `ariadne attention` lists only the daemon's shared attention items.
//! An incomplete read keeps its warning and never restores rows from raw flags.

use anyhow::Result;

use ariadne_api::attention::{AttentionCause, AttentionListDto, AttentionSubjectKind};
use ariadne_client::{Client, SseEvent};
use ariadne_core::AttentionReason;

use super::follow;
use crate::output::table::{check_columns, quiet_lines, render_table};
use crate::output::{
    Column, Format, UNCAPPED, View, age, col, empty_state, note, print_json, view,
};

/// The existing column names, applied to shared attention items.
const ROWS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 48).title(),
    col("reason", UNCAPPED).attention(),
    col("task", 40).rank(1),
    col("age", UNCAPPED).rank(2),
];

/// The session commands and event output use the same words as Desktop.
pub(crate) fn reason_label(reason: AttentionReason) -> &'static str {
    match reason {
        AttentionReason::WaitingPermission => "waiting for permission",
        AttentionReason::WaitingInput => "waiting for input",
        AttentionReason::WaitingUser => "waiting for you",
        AttentionReason::AgentError => "agent error",
        AttentionReason::Disconnected => "disconnected",
        AttentionReason::Stalled => "stalled",
        AttentionReason::Exhausted => "exhausted",
    }
}

/// Every daemon item keeps its cause, summary, action, age and affected work.
fn recovery_items_section(
    recovery: &AttentionListDto,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    if recovery.items.is_empty() {
        return None;
    }
    let mut lines = vec!["NEEDS ATTENTION".to_string()];
    for item in &recovery.items {
        lines.push(format!(
            "- [{}] {} ({} old)",
            cause_label(item.reason),
            item.summary,
            age(&item.since, now)
        ));
        lines.push(format!("  action: {}", item.required_action));
        lines.push(format!("  dismiss: ariadne attention dismiss {}", item.id));
        if let ariadne_api::attention::AttentionTarget::Console { session_id } = &item.target {
            lines.push(format!("  attach: ariadne attach {session_id}"));
        }
        // Name every affected subject. The item's target supplies its action.
        for subject in &item.affected {
            lines.push(format!("  affects: {} ({})", subject.id, subject.label));
        }
    }
    Some(lines.join("\n"))
}

/// Quiet output contains the IDs supplied by the daemon, one per line.
fn quiet_rows(attention: &AttentionListDto) -> Vec<Vec<String>> {
    attention
        .items
        .iter()
        .map(|item| vec![item.id.clone()])
        .collect()
}

/// Table cells come only from daemon items, including grouped task labels.
fn column_rows(
    attention: &AttentionListDto,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<Vec<String>> {
    attention
        .items
        .iter()
        .map(|item| {
            let tasks = item
                .affected
                .iter()
                .filter(|subject| subject.kind == AttentionSubjectKind::Task)
                .map(|subject| subject.label.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            vec![
                item.id.clone(),
                item.summary.clone(),
                cause_label(item.reason).into(),
                if tasks.is_empty() { "-".into() } else { tasks },
                age(&item.since, now),
            ]
        })
        .collect()
}

/// The table spelling of a recovery cause.
fn cause_label(cause: AttentionCause) -> &'static str {
    match cause {
        AttentionCause::Access => "access",
        AttentionCause::Quota => "quota",
        AttentionCause::Configuration => "configuration",
        AttentionCause::Resource => "resource",
        AttentionCause::Unknown => "unknown",
    }
}

/// The events that can put a row on this list or take one off it: a task's
/// status or stall flag, a session's attention flag, and a goal appearing or
/// going. Nothing else redraws it — an agent event moves the daemon, and it is
/// the daemon's own write that arrives here as a session update.
fn relevant(frame: &SseEvent) -> bool {
    matches!(
        frame.event.as_str(),
        "goal_created"
            | "goal_updated"
            | "goal_deleted"
            | "task_created"
            | "task_updated"
            | "session_created"
            | "session_updated"
            // A repository's forge fetch error is a configuration recovery
            // item's own evidence, and its removal or a goal's is what
            // takes a stale one down (`GET /v1/attention`).
            | "repository_updated"
            | "repository_deleted"
            // A reviewer session's own give-up, set or cleared, is a
            // recovery item's own evidence
            // (`PullRequestRow::reviewer_given_up_at`).
            | "pull_requests_changed"
            | "attention_changed"
    )
}

/// Whether this run prints a table, which is what makes a `--columns` worth
/// refusing.
///
/// JSON has no columns at all, and `-q` prints the first cell of every row
/// whatever `--columns` names — so neither reads the flag, and neither may
/// refuse it. Every other `-q` listing goes through [`super::super::output::print_list`],
/// which skips the table and never looks at `--columns`; refusing here alone
/// would fail one command where the rest of the CLI does not.
fn prints_a_table(format: Format, view: &View) -> bool {
    matches!(format, Format::Table) && !view.quiet
}

pub(crate) async fn run(client: &Client, watch: bool, format: Format) -> Result<()> {
    // Before the first request, let alone the first table: a `--columns` this
    // board does not have is one error, not one per goal — and on a `--watch`
    // it is an error rather than a cleared screen saying it forever.
    if prints_a_table(format, view()) {
        check_columns(ROWS, view())?;
    }
    if !watch {
        return render(client, format).await;
    }
    follow::watch(client, "/v1/events/stream", relevant, async || {
        render(client, format).await
    })
    .await
}

/// Read the shared list afresh on every redraw. Its items are authoritative
/// even when another producer could not read all of its evidence.
async fn render(client: &Client, format: Format) -> Result<()> {
    let attention: AttentionListDto = client.get_json("/v1/attention").await?;
    let now = chrono::Utc::now();
    let section = recovery_items_section(&attention, now);
    let incomplete = !attention.complete;
    match format {
        Format::Json => print_json(&serde_json::json!({
            // Keep the JSON envelope for existing scripts. Local goal rows are retired.
            "goals": [],
            "count": attention.items.len(),
            "recovery": &attention.items,
            "complete": attention.complete,
        }))?,
        Format::Table if view().quiet => {
            let rows = quiet_rows(&attention);
            if !rows.is_empty() {
                println!("{}", quiet_lines(&rows));
            }
            // Quiet output has no field for completeness, so preserve its failure.
            if incomplete {
                anyhow::bail!(
                    "some of what needs attention could not be read; the ids above are not the \
                     whole list"
                );
            }
        }
        Format::Table if section.is_none() => {
            if incomplete {
                note("Some of what needs attention could not be read; showing what is known.");
            } else {
                note(&empty_state("Nothing needs attention.", None));
            }
        }
        Format::Table => {
            if view().columns.is_empty() {
                if let Some(section) = section {
                    println!("{section}");
                }
                if incomplete {
                    println!();
                }
            } else {
                println!(
                    "{}",
                    render_table(ROWS, &column_rows(&attention, now), view())?
                );
            }
            if incomplete {
                note("Some of what needs attention could not be read; showing what is known.");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::fixtures::NOW;
    use ariadne_api::attention::AttentionSubjectKind;

    fn frame(event: &str) -> SseEvent {
        SseEvent {
            event: event.into(),
            data: String::new(),
            id: None,
        }
    }

    fn attention_item(
        reason: AttentionCause,
        affected: Vec<(AttentionSubjectKind, &str)>,
    ) -> ariadne_api::attention::AttentionItemDto {
        use ariadne_api::attention::{AttentionProducer, AttentionSubjectDto, AttentionTarget};
        ariadne_api::attention::AttentionItemDto {
            occurrence: String::new(),
            id: "01I".into(),
            producer: AttentionProducer::Recovery,
            reason,
            summary: "a blocker".into(),
            required_action: "clear it".into(),
            since: NOW.into(),
            affected: affected
                .into_iter()
                .map(|(kind, id)| AttentionSubjectDto {
                    kind,
                    id: id.into(),
                    label: id.into(),
                })
                .collect(),
            target: AttentionTarget::Settings {
                section: "forge".into(),
            },
        }
    }

    #[test]
    fn watch_redraws_on_a_goals_or_a_pull_requests_own_recovery_evidence() {
        assert!(relevant(&frame("goal_updated")));
        assert!(relevant(&frame("pull_requests_changed")));
        assert!(relevant(&frame("attention_changed")));
        assert!(!relevant(&frame("ai_permissions_updated")));
    }

    #[test]
    fn every_recovery_item_prints_in_its_own_section() {
        let now = NOW.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
        let recovery = AttentionListDto {
            items: vec![attention_item(
                AttentionCause::Quota,
                vec![(AttentionSubjectKind::Session, "01S")],
            )],
            complete: true,
        };
        let section = recovery_items_section(&recovery, now).expect("a quota item has a section");
        assert!(section.contains("[quota]"), "{section}");
        assert!(section.contains("a blocker"), "{section}");
        assert!(section.contains("clear it"), "{section}");
        assert!(section.contains("01S"), "{section}");

        assert_eq!(
            recovery_items_section(
                &AttentionListDto {
                    items: Vec::new(),
                    complete: true
                },
                now
            ),
            None
        );
    }

    #[test]
    fn a_bad_columns_flag_is_refused_before_any_table() {
        let bad = View {
            columns: vec!["colour".into()],
            ..View::plain()
        };
        let err = check_columns(ROWS, &bad).expect_err("no such column");
        assert!(err.to_string().contains("no column \"colour\""), "{err}");
        check_columns(ROWS, &View::plain()).expect("no --columns names nothing wrong");
    }

    #[test]
    fn a_columns_flag_is_refused_only_where_a_table_is_printed() {
        let quiet = View {
            quiet: true,
            ..View::plain()
        };
        assert!(prints_a_table(Format::Table, &View::plain()));
        assert!(!prints_a_table(Format::Table, &quiet));
        assert!(!prints_a_table(Format::Json, &View::plain()));
        assert!(!prints_a_table(Format::Json, &quiet));
    }

    #[test]
    fn quiet_rows_also_names_every_recovery_item() {
        let recovery = AttentionListDto {
            items: vec![attention_item(
                AttentionCause::Configuration,
                vec![(AttentionSubjectKind::Repository, "01R")],
            )],
            complete: true,
        };
        assert_eq!(quiet_lines(&quiet_rows(&recovery)), "01I");
    }

    #[test]
    fn session_reason_labels_match_desktop() {
        for (reason, label) in [
            (AttentionReason::WaitingPermission, "waiting for permission"),
            (AttentionReason::WaitingInput, "waiting for input"),
            (AttentionReason::WaitingUser, "waiting for you"),
            (AttentionReason::AgentError, "agent error"),
            (AttentionReason::Disconnected, "disconnected"),
            (AttentionReason::Stalled, "stalled"),
            (AttentionReason::Exhausted, "exhausted"),
        ] {
            assert_eq!(reason_label(reason), label);
        }
    }
}
