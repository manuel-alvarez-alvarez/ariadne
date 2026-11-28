//! The tabs of the console (008, rule 53): `main`, the session's own
//! transcript, and one tab for each subagent of the session, in start order.
//! While the session has a subagent, the bar of the tabs is the top row.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use ariadne_api::trajectory::SubagentStepDto;

use crate::wrap;

use super::{App, Item, StepDto, draw::subagent_mark};

/// What the bar calls the session's own transcript.
const MAIN: &str = "main";
/// The fewest columns a cut name keeps. Where the tabs do not fit with
/// their names cut to it, the bar shows the tabs round the active one.
const LEAST_NAME: usize = 4;
/// The marks of tabs past the left and the right edge of the bar.
const MORE_LEFT: &str = "‹";
const MORE_RIGHT: &str = "›";

/// Where a tab was left: its first line on the screen and its cursor.
#[derive(Debug, Default)]
pub(super) struct Place {
    scroll: Option<usize>,
    cursor: Option<Item>,
}

/// One tab of the bar: the state mark of its subagent, where it has one, its
/// name and whether it is on the screen.
struct Tab {
    mark: Option<(&'static str, Style)>,
    name: String,
    active: bool,
}

impl Tab {
    /// The columns the tab draws with its name cut to `name` columns.
    fn width(&self, name: usize) -> usize {
        let cut = self.cut(name);
        let mark = self.mark.map_or(0, |(mark, _)| {
            wrap::width(mark) + usize::from(!cut.is_empty())
        });
        2 + mark + wrap::width(&cut)
    }

    /// The name cut to `name` columns: none at all where it has no room.
    fn cut(&self, name: usize) -> String {
        match name {
            0 => String::new(),
            name => wrap::clip(&self.name, name),
        }
    }

    /// The columns the name of the tab has where the names of subagents
    /// have `name`: `main` is cut only for the active tab alone.
    fn room(&self, name: usize) -> usize {
        match self.mark {
            Some(_) => name,
            None => name.max(LEAST_NAME),
        }
    }

    /// The most columns the name has where the tab must fit `room`.
    fn fit(&self, room: usize) -> usize {
        let mark = self.mark.map_or(0, |(mark, _)| wrap::width(mark) + 1);
        let most = room.saturating_sub(2 + mark);
        (0..=most)
            .rev()
            .find(|name| self.width(*name) <= room)
            .unwrap_or(0)
    }

    /// The tab, bracketed and reversed while it is on the screen.
    fn spans(&self, name: usize) -> Vec<Span<'static>> {
        let (open, close) = match self.active {
            true => ("[", "]"),
            false => (" ", " "),
        };
        let shown = match self.active {
            true => Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            false => Style::new(),
        };
        let cut = self.cut(name);
        let mut spans = vec![Span::styled(open, shown)];
        if let Some((mark, style)) = self.mark {
            spans.push(Span::styled(mark, style.patch(shown)));
            if !cut.is_empty() {
                spans.push(Span::styled(" ", shown));
            }
        }
        spans.push(Span::styled(cut, shown));
        spans.push(Span::styled(close, shown));
        spans
    }
}

impl App {
    /// The subagents of the session, in start order.
    pub(super) fn subagents(&self) -> Vec<&SubagentStepDto> {
        let mut subagents: Vec<&SubagentStepDto> = self
            .view
            .as_ref()
            .map(|view| view.subagents().iter().collect())
            .unwrap_or_default();
        subagents.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        subagents
    }

    /// The subagent of the tab on the screen. `None` in `main`.
    pub(super) fn shown_subagent(&self) -> Option<&SubagentStepDto> {
        let id = self.tab.as_deref()?;
        self.view
            .as_ref()?
            .subagents()
            .iter()
            .find(|subagent| subagent.subagent_id == id)
    }

    /// Whether the session has a tab for the lane `lane`.
    pub(super) fn has_tab(&self, lane: &str) -> bool {
        self.view.as_ref().is_some_and(|view| {
            view.subagents()
                .iter()
                .any(|subagent| subagent.subagent_id == lane)
        })
    }

    /// The tab a summary row opens: the subagent of the step at `item`,
    /// where the session has a tab for it.
    pub(super) fn tab_of(&self, item: &Item) -> Option<String> {
        let Item::Step { turn, step } = item else {
            return None;
        };
        match self.step(turn, step)? {
            StepDto::Subagent(subagent) if self.has_tab(&subagent.subagent_id) => {
                Some(subagent.subagent_id.clone())
            }
            _ => None,
        }
    }

    /// Put the next tab on the screen, or the one before. The ends wrap.
    pub(super) fn cycle(&mut self, forward: bool) {
        let tabs: Vec<Option<String>> = std::iter::once(None)
            .chain(
                self.subagents()
                    .into_iter()
                    .map(|subagent| Some(subagent.subagent_id.clone())),
            )
            .collect();
        if tabs.len() < 2 {
            return;
        }
        let at = tabs.iter().position(|tab| *tab == self.tab).unwrap_or(0);
        let to = match forward {
            true => (at + 1) % tabs.len(),
            false => (at + tabs.len() - 1) % tabs.len(),
        };
        self.show(tabs[to].clone());
    }

    /// Put the tab `to` on the screen, where it was left: the tab it
    /// replaces keeps its scroll and its cursor.
    pub(super) fn show(&mut self, to: Option<String>) {
        if to == self.tab {
            return;
        }
        let left = Place {
            scroll: self.scroll.get(),
            cursor: self.cursor.take(),
        };
        self.places.insert(self.tab.take(), left);
        let place = self.places.remove(&to).unwrap_or_default();
        self.scroll.set(place.scroll);
        self.cursor = place.cursor;
        self.tab = to;
        self.settle();
    }

    /// The bar of the tabs at `width`, while the session has a subagent.
    /// Names are cut before a tab leaves the bar, and the state marks stay.
    /// From five columns, the active tab, its mark and the marks of the
    /// tabs past the edges fit whole.
    pub(super) fn tab_bar(&self, width: usize) -> Option<Line<'static>> {
        let subagents = self.subagents();
        if subagents.is_empty() {
            return None;
        }
        let mut tabs = vec![Tab {
            mark: None,
            name: MAIN.to_string(),
            active: self.tab.is_none(),
        }];
        tabs.extend(subagents.iter().map(|subagent| {
            Tab {
                mark: Some(subagent_mark(subagent)),
                name: subagent
                    .title
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string(),
                active: self.tab.as_deref() == Some(subagent.subagent_id.as_str()),
            }
        }));
        let longest = tabs
            .iter()
            .skip(1)
            .map(|tab| wrap::width(&tab.name))
            .max()
            .unwrap_or(0);
        let total = |name: usize| {
            tabs.iter()
                .map(|tab| tab.width(tab.room(name)))
                .sum::<usize>()
        };
        let least = LEAST_NAME.min(longest);
        if let Some(name) = (least..=longest).rev().find(|name| total(*name) <= width) {
            return Some(Line::from(
                tabs.iter()
                    .flat_map(|tab| tab.spans(tab.room(name)))
                    .collect::<Vec<_>>(),
            ));
        }
        // The tabs round the active one, out to the right and then to the
        // left, with a column on each side for the mark of more tabs. The
        // active tab cuts its name further where it alone does not fit.
        let room = width.saturating_sub(2);
        let active = tabs.iter().position(|tab| tab.active).unwrap_or(0);
        let shown = tabs[active].room(least).min(tabs[active].fit(room));
        let name = |at: usize| match at == active {
            true => shown,
            false => tabs[at].room(least),
        };
        let (mut from, mut to) = (active, active + 1);
        let mut used = tabs[active].width(shown);
        while to < tabs.len() && used + tabs[to].width(name(to)) <= room {
            used += tabs[to].width(name(to));
            to += 1;
        }
        while from > 0 && used + tabs[from - 1].width(name(from - 1)) <= room {
            from -= 1;
            used += tabs[from].width(name(from));
        }
        let mut spans = vec![Span::raw(if from > 0 { MORE_LEFT } else { " " })];
        spans.extend((from..to).flat_map(|at| tabs[at].spans(name(at))));
        if to < tabs.len() {
            spans.push(Span::raw(MORE_RIGHT));
        }
        Some(Line::from(spans))
    }
}
