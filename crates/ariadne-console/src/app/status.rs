//! The live state of the session around the input: the plan line above it,
//! which opens to the checklist, and the bottom row under the input.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use ariadne_api::trajectory::{PlanDto, SessionStateDto};

use crate::theme::{self, AGENT, ASK, DIM, FAIL, PLAN, TOOL};
use crate::wrap::{self, wrap};

use super::draw::{FOLDED, UNFOLDED};

/// The plan above the input: one line while it is closed, and the line with
/// the checklist under it while it is open, no more than `most` rows. A
/// checklist taller than its room shows the entry lines from `scroll` on,
/// and a last row that counts the lines above and below. The answer holds
/// `scroll` kept in range, and how many entry lines show.
pub(super) fn plan_lines(
    plan: &PlanDto,
    open: bool,
    scroll: usize,
    width: usize,
    most: usize,
) -> (Vec<Line<'static>>, usize, usize) {
    if most == 0 {
        return (Vec::new(), 0, 0);
    }
    let done = plan
        .entries
        .iter()
        .filter(|entry| entry.status == "completed")
        .count();
    let current = plan
        .entries
        .iter()
        .find(|entry| entry.status == "in_progress")
        .or_else(|| {
            plan.entries
                .iter()
                .find(|entry| entry.status != "completed")
        })
        .map(|entry| entry.content.clone())
        .unwrap_or_default();
    let lead = format!(
        "{}plan {done}/{}{}",
        if open { UNFOLDED } else { FOLDED },
        plan.entries.len(),
        theme::SEPARATOR
    );
    let room = width.saturating_sub(wrap::width(&lead));
    let mut lines = vec![Line::from(vec![
        Span::styled(lead, PLAN),
        Span::raw(wrap::clip(current.lines().next().unwrap_or_default(), room)),
    ])];
    if !open {
        return (lines, 0, 0);
    }
    let mut entries = Vec::new();
    for entry in &plan.entries {
        let (mark, style) = match entry.status.as_str() {
            "completed" => (theme::PLAN_DONE, DIM),
            "in_progress" => (theme::PLAN_DOING, TOOL),
            _ => (theme::PLAN_TODO, Style::new()),
        };
        let room = width.saturating_sub(wrap::width(mark));
        for (at, part) in wrap(&entry.content, room).into_iter().enumerate() {
            let lead = if at == 0 {
                mark.to_string()
            } else {
                " ".repeat(wrap::width(mark))
            };
            entries.push(Line::from(vec![
                Span::styled(lead, style),
                Span::styled(part, style),
            ]));
        }
    }
    if entries.len() < most {
        let shown = entries.len();
        lines.extend(entries);
        return (lines, 0, shown);
    }
    // The head and the count take two rows; the entries get the rest, and
    // at least one.
    let shown = most.saturating_sub(2).max(1);
    let scroll = scroll.min(entries.len() - shown);
    let below = entries.len() - shown - scroll;
    lines.extend(entries.into_iter().skip(scroll).take(shown));
    if lines.len() < most {
        lines.push(Line::styled(
            wrap::clip(
                &format!("  {scroll} above · {below} below · the wheel scrolls the plan"),
                width,
            ),
            DIM,
        ));
    }
    (lines, scroll, shown)
}

/// The mark of the session's state: it says what a status word said.
fn state_mark(status: &str) -> (&'static str, Style) {
    match status {
        "running" | "starting" => (theme::CALL_RUNNING, TOOL),
        "failed" => (theme::CALL_FAILED, FAIL),
        "idle" => (theme::SESSION_IDLE, AGENT),
        _ => (theme::SESSION_ENDED, DIM),
    }
}

/// The one row under the input box. On the left: the mark of the session's
/// state, its model — the agent's model option, else the model the session
/// was launched with — its mode, the context it uses and its cost. On the
/// right: the hint that `?` lists the keys. `said` replaces the left side
/// while it applies. Where the row does not fit, it drops whole items, the
/// least important first: the cost, the context, the mode, the model, the
/// mark, and `? keys` last.
pub(super) fn bottom_row(
    session: Option<&SessionStateDto>,
    stored_model: Option<&str>,
    said: Option<&str>,
    width: usize,
) -> Line<'static> {
    // Each item of the left side, in its place on the row, with its rank:
    // the lowest rank goes first.
    let mut items: Vec<(u8, String, Style)> = Vec::new();
    if let Some(said) = said {
        items.push((5, said.to_string(), ASK));
    } else if let Some(session) = session {
        let (mark, style) = state_mark(session.status.as_str());
        items.push((4, mark.to_string(), style));
        if let Some(model) = session
            .config_options
            .iter()
            .find(|option| option.category.as_deref() == Some("model"))
            .and_then(|option| {
                let value = option.current_value.as_deref()?;
                Some(
                    option
                        .values
                        .iter()
                        .find(|held| held.value == value)
                        .map_or(value.to_string(), |held| held.name.clone()),
                )
            })
            .or_else(|| stored_model.map(str::to_string))
        {
            items.push((3, model, DIM));
        }
        if let Some(mode) = session.mode.as_ref().and_then(|mode| {
            let id = mode.current_id.as_deref()?;
            Some(
                mode.available
                    .iter()
                    .find(|available| available.id == id)
                    .map_or(id.to_string(), |available| available.name.clone()),
            )
        }) {
            items.push((2, mode, DIM));
        }
        if let Some(context) = session.context.filter(|context| context.size > 0) {
            let percent = context.used.saturating_mul(100) / context.size;
            items.push((1, format!("context {percent}%"), DIM));
        }
        if let Some(cost) = &session.cost {
            let said = match cost.currency.as_str() {
                "USD" => format!("${:.2}", cost.amount),
                currency => format!("{:.2} {currency}", cost.amount),
            };
            items.push((0, said, DIM));
        }
    }
    let keys = wrap::width(theme::KEYS_HINT);
    // A margin on both sides, and two columns between the sides.
    let fits = |items: &[(u8, String, Style)], hint: bool| {
        let texts: usize = items.iter().map(|(_, text, _)| wrap::width(text)).sum();
        let gaps = wrap::width(theme::SEPARATOR) * items.len().saturating_sub(1);
        let hint = if hint { keys + 3 } else { 0 };
        1 + texts + gaps + hint <= width
    };
    let mut hint = true;
    while !fits(&items, hint) && (hint || !items.is_empty()) {
        if items.is_empty() {
            hint = false;
            continue;
        }
        let least = items
            .iter()
            .enumerate()
            .min_by_key(|(_, (rank, ..))| *rank)
            .map(|(at, _)| at)
            .unwrap_or_default();
        items.remove(least);
    }
    let mut spans = vec![Span::raw(" ")];
    for (at, (_, text, style)) in items.into_iter().enumerate() {
        if at > 0 {
            spans.push(Span::styled(theme::SEPARATOR, DIM));
        }
        spans.push(Span::styled(text, style));
    }
    if hint {
        let left: usize = spans.iter().map(|span| wrap::width(&span.content)).sum();
        let pad = width.saturating_sub(left + keys + 1);
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(theme::KEYS_HINT, DIM));
    }
    let line = Line::from(spans);
    match line.width() <= width {
        true => line,
        false => Line::styled(wrap::clip(&line.to_string(), width), DIM),
    }
}
