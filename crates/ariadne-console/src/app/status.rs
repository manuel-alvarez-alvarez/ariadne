//! The live state of the session around the input: the plan line above it,
//! which opens to the checklist, and the status row under it.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use ariadne_api::trajectory::{PlanDto, SessionStateDto};

use crate::theme::{self, AGENT, DIM, FAIL, PLAN, TOOL};
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

/// The status row: the session's status, its mode, its model — the agent's
/// model option, else the model the session was launched with — the context
/// it uses, its cost and its tokens. Where the row does not fit, it drops
/// whole items, the least important first: the cost, then the tokens, the
/// context, the mode and the model. The status stays to the last.
pub(super) fn status_line(
    session: &SessionStateDto,
    stored_model: Option<&str>,
    width: usize,
) -> Line<'static> {
    // Each item, in its place on the row, with its rank: the lowest rank
    // goes first.
    let mut items: Vec<(u8, String, Style)> = Vec::new();
    let status = session.status.as_str();
    let status_style = match status {
        "running" | "starting" => TOOL,
        "failed" => FAIL,
        "idle" => AGENT,
        _ => DIM,
    };
    items.push((5, status.to_string(), status_style));
    if let Some(mode) = session.mode.as_ref().and_then(|mode| {
        let id = mode.current_id.as_deref()?;
        Some(
            mode.available
                .iter()
                .find(|available| available.id == id)
                .map_or(id.to_string(), |available| available.name.clone()),
        )
    }) {
        items.push((3, mode, DIM));
    }
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
        items.push((4, model, DIM));
    }
    if let Some(context) = session.context.filter(|context| context.size > 0) {
        let percent = context.used.saturating_mul(100) / context.size;
        items.push((2, format!("context {percent}%"), DIM));
    }
    if let Some(cost) = &session.cost {
        let said = match cost.currency.as_str() {
            "USD" => format!("${:.2}", cost.amount),
            currency => format!("{:.2} {currency}", cost.amount),
        };
        items.push((0, said, DIM));
    }
    let usage = session.usage;
    if usage.input_tokens > 0 || usage.output_tokens > 0 {
        items.push((
            1,
            format!(
                "{}{} {}{}",
                theme::TOKENS_IN,
                compact(usage.input_tokens),
                theme::TOKENS_OUT,
                compact(usage.output_tokens)
            ),
            DIM,
        ));
    }
    let fits = |items: &[(u8, String, Style)]| {
        let texts: usize = items.iter().map(|(_, text, _)| wrap::width(text)).sum();
        let gaps = wrap::width(theme::SEPARATOR) * items.len().saturating_sub(1);
        1 + texts + gaps <= width
    };
    while items.len() > 1 && !fits(&items) {
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
    let line = Line::from(spans);
    match line.width() <= width {
        true => line,
        // The status alone is wider than the row.
        false => Line::styled(wrap::clip(&line.to_string(), width), DIM),
    }
}

/// A count of tokens in at most five columns: `950`, `12.4k`, `1.2M`.
fn compact(count: u64) -> String {
    let tenths = |unit: u64| (count + unit / 20) / (unit / 10);
    let (tenths, suffix) = match count {
        0..1_000 => return count.to_string(),
        _ if tenths(1_000) < 10_000 => (tenths(1_000), "k"),
        _ => (tenths(1_000_000), "M"),
    };
    match tenths % 10 {
        0 => format!("{}{suffix}", tenths / 10),
        tenth => format!("{}.{tenth}{suffix}", tenths / 10),
    }
}
