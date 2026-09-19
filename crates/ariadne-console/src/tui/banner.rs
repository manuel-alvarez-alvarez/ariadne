//! The short session identity banner printed into scrollback as a console opens.

use ratatui::text::Line;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::Header;
use crate::theme;

const FRAME_AT: usize = 40;

/// Draw the session identity at `width` columns.
pub(super) fn draw(header: &Header, width: usize) -> Vec<Line<'static>> {
    let mut fields = vec![("", format!("ariadne · {}", header.seat))];
    if let Some(task) = &header.task {
        fields.push(("task     ", task.clone()));
    }
    if let Some(model) = &header.model {
        let model = match &header.effort {
            Some(effort) => format!("{model} · {effort}"),
            None => model.clone(),
        };
        fields.push(("model    ", model));
    }
    if let Some(repository) = &header.repository {
        fields.push(("repo     ", repository.clone()));
    }
    if let Some(id) = &header.id {
        fields.push(("session  ", clip(id, 12)));
    }

    if width < FRAME_AT {
        return fields
            .into_iter()
            .map(|(label, value)| {
                Line::styled(
                    format!(
                        "{label}{}",
                        clip(&value, width.saturating_sub(label.width() + 1))
                    ),
                    theme::BANNER,
                )
            })
            .collect();
    }

    // Ratatui's inline viewport needs one cell at the right edge for its
    // cursor even when the banner itself has no more text to draw.
    let room = width.saturating_sub(5);
    let body: Vec<String> = fields
        .into_iter()
        .map(|(label, value)| {
            format!(
                "{label}{}",
                clip(&value, room.saturating_sub(label.width()))
            )
        })
        .collect();
    let inside = body
        .iter()
        .map(|line| line.width())
        .max()
        .unwrap_or_default()
        .min(room);
    let mut lines = Vec::with_capacity(body.len() + 2);
    lines.push(Line::styled(
        format!(
            "{}{}{}",
            theme::BANNER_TOP_LEFT,
            theme::BANNER_HORIZONTAL.repeat(inside + 2),
            theme::BANNER_TOP_RIGHT
        ),
        theme::BANNER,
    ));
    lines.extend(body.into_iter().map(|line| {
        Line::styled(
            format!(
                "{} {}{} {}",
                theme::BANNER_VERTICAL,
                line,
                " ".repeat(inside.saturating_sub(line.width())),
                theme::BANNER_VERTICAL
            ),
            theme::BANNER,
        )
    }));
    lines.push(Line::styled(
        format!(
            "{}{}{}",
            theme::BANNER_BOTTOM_LEFT,
            theme::BANNER_HORIZONTAL.repeat(inside + 2),
            theme::BANNER_BOTTOM_RIGHT
        ),
        theme::BANNER,
    ));
    lines
}

/// Cut at a grapheme boundary and account for display width.
fn clip(text: &str, room: usize) -> String {
    if text.width() <= room {
        return text.to_string();
    }
    let mut cut = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let width = grapheme.width();
        if used + width > room.saturating_sub(1) {
            break;
        }
        used += width;
        cut.push_str(grapheme);
    }
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::header;

    #[test]
    fn a_long_title_is_cut_inside_an_eighty_column_frame() {
        let mut header = header();
        header.task = Some("x".repeat(200));
        let lines = draw(&header, 80);
        assert!(lines.iter().any(|line| line.to_string().contains('…')));
        assert!(lines.iter().all(|line| line.width() <= 80));
    }

    #[test]
    fn a_narrow_pane_has_no_frame() {
        let lines = draw(&header(), 30);
        assert!(
            lines
                .iter()
                .all(|line| !line.to_string().contains(theme::BANNER_VERTICAL))
        );
    }

    #[test]
    fn missing_context_omits_its_lines() {
        let mut header = header();
        header.task = None;
        header.repository = None;
        let shown = draw(&header, 80)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!shown.contains("task     "));
        assert!(!shown.contains("repo     "));
    }

    #[test]
    fn an_unread_session_omits_values_the_host_does_not_have() {
        let shown = draw(&Header::of(None), 80)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!shown.contains("model    "));
        assert!(!shown.contains("session  "));
    }

    #[test]
    fn orchestrator_context_uses_the_goal_title() {
        let mut header = header();
        header.seat = "orchestrator".into();
        header.task = Some("TUI improvements".into());
        let shown = draw(&header, 80)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(shown.contains("ariadne · orchestrator"));
        assert!(shown.contains("task     TUI improvements"));
    }

    #[test]
    fn the_banner_precedes_the_first_block_and_a_snapshot_does_not_repeat_it() {
        let mut console = crate::tui::Console::new(header());
        let mut terminal = crate::tui::testing::terminal();
        console.banner(&mut terminal).unwrap();
        console.apply(&crate::tui::testing::event(
            "user_prompt_submit",
            "first",
            serde_json::json!({"text": "first", "source": "console"}),
        ));
        console.apply(&crate::tui::testing::event(
            "agent_message",
            "next",
            serde_json::json!({"text": "next"}),
        ));
        console.commit(&mut terminal).unwrap();
        console.snapshot(&[]);
        let shown = crate::tui::testing::screen(&terminal);
        assert_eq!(shown.matches("ariadne · author").count(), 1, "{shown}");
        assert!(
            shown.find("ariadne · author").unwrap() < shown.find("> first").unwrap(),
            "{shown}"
        );
    }
}
