//! The prompts typed at a console, which the box's up and down keys walk.

use std::future::Future;

use ariadne_api::trajectory::{TrajectorySnapshotDto, TurnDto};

use crate::trajectory::text;

/// How many turns a host asks for in one page of older turns.
pub const PAGE: u32 = 50;

/// How many pages of older turns a host reads, at most: the newest snapshot
/// and these reach back a few hundred prompts, and no further.
const PAGES: usize = 4;

/// The prompts typed at a console in `turns`, oldest first.
pub(crate) fn typed(turns: &[TurnDto]) -> Vec<String> {
    turns
        .iter()
        .filter(|turn| from_console(turn))
        .filter_map(prompt_text)
        .collect()
}

/// The prompts typed at a console in a snapshot and in the pages of turns
/// older than it, oldest first.
///
/// A snapshot holds the newest turns only. `older` reads the page of
/// [`PAGE`] turns before the turn it is given, and answers `None` where the
/// read failed, which ends the reach with what it has.
pub async fn reach_back<F, Fut>(snapshot: &TrajectorySnapshotDto, mut older: F) -> Vec<String>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Option<TrajectorySnapshotDto>>,
{
    let mut prompts = typed(&snapshot.turns);
    let mut first = snapshot.turns.first().map(|turn| turn.id.clone());
    let mut more = snapshot.has_older;
    for _ in 0..PAGES {
        let (true, Some(before)) = (more, first.take()) else {
            break;
        };
        let Some(page) = older(before).await else {
            break;
        };
        prompts.splice(0..0, typed(&page.turns));
        first = page.turns.first().map(|turn| turn.id.clone());
        more = page.has_older;
    }
    prompts
}

/// Whether a turn's prompt was typed at a console.
pub(super) fn from_console(turn: &TurnDto) -> bool {
    turn.prompt.as_ref().is_some_and(|prompt| {
        prompt
            .source
            .as_deref()
            .is_none_or(|source| source == "console")
    })
}

/// The text a turn's prompt was submitted with.
pub(super) fn prompt_text(turn: &TurnDto) -> Option<String> {
    let prompt = turn.prompt.as_ref()?;
    Some(prompt.text.clone().unwrap_or_else(|| text(&prompt.content)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::reach_back;
    use crate::trajectory::fixtures::{snapshot, turn};

    #[tokio::test]
    async fn the_history_reaches_back_through_the_older_pages_oldest_first() {
        let mut newest = snapshot(
            1,
            vec![
                turn("t3", "completed", json!({})),
                turn("t4", "running", json!({})),
            ],
        );
        newest.has_older = true;
        let mut older = snapshot(
            0,
            vec![
                turn("t1", "completed", json!({})),
                turn(
                    "t2",
                    "completed",
                    json!({"prompt": {"id": "t2", "source": "daemon", "prompt_kind": "agent_resume", "content": []}}),
                ),
            ],
        );
        older.has_older = false;
        let asked = std::cell::RefCell::new(Vec::new());

        let prompts = reach_back(&newest, |before| {
            asked.borrow_mut().push(before);
            let page = older.clone();
            async move { Some(page) }
        })
        .await;

        assert_eq!(prompts, ["prompt t1", "prompt t3", "prompt t4"]);
        assert_eq!(
            *asked.borrow(),
            ["t3"],
            "the page older than the first turn, once"
        );
    }

    #[tokio::test]
    async fn a_page_that_cannot_be_read_ends_the_reach_with_what_it_has() {
        let mut newest = snapshot(1, vec![turn("t3", "completed", json!({}))]);
        newest.has_older = true;

        let prompts = reach_back(&newest, |_| async { None }).await;

        assert_eq!(prompts, ["prompt t3"]);
    }
}
