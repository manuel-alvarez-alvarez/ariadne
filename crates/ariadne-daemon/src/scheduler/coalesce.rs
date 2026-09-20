//! The session wakes of one burst, folded into a reconcile at each end of it.
//!
//! An agent reports every event it has — one tool call is a start and an end —
//! and each of those woke a full reconciliation of the task it belongs to. A
//! production database holds 24,679 tool ends, and eight agents reporting at
//! once read the store out of connections, so a reviewer could not be started.
//!
//! What this holds is a window per session. The first wake of a burst is
//! reconciled at once, so nothing the user waits on waits on a window; every
//! wake inside the window is folded into one more reconcile at the end of it,
//! so no wake is lost. A window that ends with nothing folded into it closes,
//! and the next wake is again immediate.
//!
//! A window runs from the end of a reconcile rather than from the wake that
//! asked for it ([`Coalesced::reconciled`]). The load this is about is the
//! load that makes a reconcile slow, and a window measured from the wake is
//! already over when a slow one returns: the wakes that queued while it ran
//! would then be reconciled one by one, which is the pass-per-event this
//! module exists to stop.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tokio::time::Instant;

pub(super) struct Coalesced {
    /// How long one window is ([`crate::timeouts::Timeouts::session_wake`]).
    window: Duration,
    /// When each open window ends, by session id.
    open: HashMap<String, Instant>,
    /// The sessions that reported again while their window was open, and are
    /// owed the reconcile at the end of it.
    folded: HashSet<String>,
}

impl Coalesced {
    pub(super) fn new(window: Duration) -> Self {
        Self {
            window,
            open: HashMap::new(),
            folded: HashSet::new(),
        }
    }

    /// Take one wake, and answer whether to reconcile the session now.
    ///
    /// The first wake of a burst opens a window and is reconciled at once.
    /// Every wake inside that window is folded into [`Self::due`].
    pub(super) fn wake(&mut self, session: &str, now: Instant) -> bool {
        if self.open.contains_key(session) {
            self.folded.insert(session.to_string());
            return false;
        }
        self.open.insert(session.to_string(), now + self.window);
        true
    }

    /// A reconcile of this session is over: its window runs from here.
    ///
    /// Called after every reconcile the loop runs, whichever asked for it.
    /// A reconcile that took longer than a window would otherwise hand the
    /// wakes that queued behind it a window that is already over, and each of
    /// them would run a pass of its own.
    pub(super) fn reconciled(&mut self, session: &str, now: Instant) {
        self.open.insert(session.to_string(), now + self.window);
    }

    /// When the earliest open window ends, if one is open. Nothing is owed
    /// before then.
    pub(super) fn next_window_end(&self) -> Option<Instant> {
        self.open.values().min().copied()
    }

    /// The sessions whose window has ended with a wake folded into it.
    ///
    /// Each of them opens a window again, so an agent that reports without a
    /// pause still costs one reconcile per window rather than one per event.
    pub(super) fn due(&mut self, now: Instant) -> Vec<String> {
        let ended: Vec<String> = self
            .open
            .iter()
            .filter(|(_, end)| **end <= now)
            .map(|(session, _)| session.clone())
            .collect();
        let mut due = Vec::new();
        for session in ended {
            if self.folded.remove(&session) {
                self.open.insert(session.clone(), now + self.window);
                due.push(session);
            } else {
                self.open.remove(&session);
            }
        }
        due
    }

    /// Every session owed a reconcile, taken now. The windows stay open, and
    /// end as they would have.
    ///
    /// What a flush does: a test that waits for the scheduler to catch up
    /// must not wait for a window as well.
    pub(super) fn take_all(&mut self) -> Vec<String> {
        self.folded.drain().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_millis(250);

    /// How many reconciles a burst of `events` wakes for one session, where
    /// the whole burst is waiting in the channel and each pass takes `pass`.
    ///
    /// Driven the way the loop drives it: a wake it answers `true` to is
    /// reconciled and then stamped, and a window that has ended is answered
    /// before the channel is read again — the worst of the orders the loop
    /// can pick, and the one a pass longer than a window leaves behind.
    fn reconciles_for_a_burst_of(events: usize, pass: Duration) -> usize {
        let mut coalesced = Coalesced::new(WINDOW);
        let mut now = Instant::now();
        let mut reconciles = 0;
        let mut queued = events;
        loop {
            if coalesced.next_window_end().is_some_and(|end| end <= now) {
                for session in coalesced.due(now) {
                    now += pass;
                    reconciles += 1;
                    coalesced.reconciled(&session, now);
                }
                continue;
            }
            if queued > 0 {
                queued -= 1;
                if coalesced.wake("session", now) {
                    now += pass;
                    reconciles += 1;
                    coalesced.reconciled("session", now);
                }
                continue;
            }
            match coalesced.next_window_end() {
                Some(end) => now = end,
                None => return reconciles,
            }
        }
    }

    /// The same for a burst whose passes cost nothing.
    fn reconciles_for_a_burst(events: usize) -> usize {
        reconciles_for_a_burst_of(events, Duration::ZERO)
    }

    /// The measurement the change is for: one tool call of one agent used to
    /// be one full reconcile, so a burst of 100 events cost 100 of them.
    #[test]
    fn a_burst_of_a_hundred_events_costs_two_reconciles() {
        assert_eq!(reconciles_for_a_burst(100), 2);
    }

    /// And the number does not grow with the burst: two, whatever its size.
    #[test]
    fn the_number_of_reconciles_does_not_grow_with_the_burst() {
        assert_eq!(reconciles_for_a_burst(1_000), 2);
        assert_eq!(reconciles_for_a_burst(2), 2);
    }

    /// And it does not grow when every pass is slower than a window, which is
    /// the load this is for: a store that answers slowly is what makes a pass
    /// slow, and a window measured from the wake rather than from the end of
    /// the pass is already over when the pass returns — which gives back the
    /// reconcile per event this module removes.
    #[test]
    fn a_burst_behind_passes_slower_than_a_window_still_costs_two_reconciles() {
        assert_eq!(reconciles_for_a_burst_of(100, WINDOW * 2), 2);
    }

    /// One event on its own is reconciled at once, and costs nothing after.
    #[test]
    fn a_single_event_is_reconciled_at_once() {
        assert_eq!(reconciles_for_a_burst(1), 1);
    }

    /// A turn ends with an event like any other, so what must not happen is
    /// that event waiting for a window: a wake that arrives once the burst
    /// before it has settled is reconciled where it lands.
    #[test]
    fn a_wake_after_a_burst_has_settled_is_reconciled_at_once() {
        let mut coalesced = Coalesced::new(WINDOW);
        let start = Instant::now();
        coalesced.wake("session", start);
        coalesced.wake("session", start);

        assert_eq!(
            coalesced.due(start + WINDOW),
            vec!["session".to_string()],
            "the burst's own last wake"
        );
        assert!(
            coalesced.due(start + WINDOW * 2).is_empty(),
            "and the burst has settled"
        );
        assert!(
            coalesced.wake("session", start + WINDOW * 3),
            "the turn that ends after it is not held back"
        );
    }

    /// The last wake of a burst is the one the state after the burst depends
    /// on: it is reconciled when the window ends, not dropped.
    #[test]
    fn the_last_wake_of_a_burst_is_reconciled_when_the_window_ends() {
        let mut coalesced = Coalesced::new(WINDOW);
        let start = Instant::now();

        assert!(
            coalesced.wake("session", start),
            "the first wake is at once"
        );
        assert!(!coalesced.wake("session", start + WINDOW / 2));
        let last = start + WINDOW - Duration::from_millis(1);
        assert!(!coalesced.wake("session", last));

        assert_eq!(
            coalesced.due(start + WINDOW),
            vec!["session".to_string()],
            "the wakes inside the window are owed one reconcile"
        );
    }

    /// A session reporting without a pause costs one reconcile per window,
    /// rather than one at the first window and none after.
    #[test]
    fn a_session_that_keeps_reporting_is_reconciled_every_window() {
        let mut coalesced = Coalesced::new(WINDOW);
        let start = Instant::now();
        coalesced.wake("session", start);

        for window in 1..=3 {
            let inside = start + WINDOW * window - WINDOW / 2;
            assert!(!coalesced.wake("session", inside));
            assert_eq!(
                coalesced.due(start + WINDOW * window),
                vec!["session".to_string()],
                "window {window} ends with the reconcile it folded"
            );
        }
    }

    /// One session's burst does not delay another session's first wake.
    #[test]
    fn each_session_has_a_window_of_its_own() {
        let mut coalesced = Coalesced::new(WINDOW);
        let start = Instant::now();

        assert!(coalesced.wake("busy", start));
        assert!(!coalesced.wake("busy", start));
        assert!(coalesced.wake("quiet", start), "another session is at once");

        let mut due = coalesced.due(start + WINDOW);
        due.sort();
        assert_eq!(due, vec!["busy".to_string()], "only the busy one folded");
    }

    /// Nothing is owed while no window is open, and the earliest end is what
    /// the loop sleeps until.
    #[test]
    fn the_next_window_end_is_the_earliest_open_one() {
        let mut coalesced = Coalesced::new(WINDOW);
        let start = Instant::now();
        assert_eq!(coalesced.next_window_end(), None, "no window is open");

        coalesced.wake("later", start + Duration::from_millis(10));
        coalesced.wake("sooner", start);
        assert_eq!(coalesced.next_window_end(), Some(start + WINDOW));

        assert!(coalesced.due(start + WINDOW).is_empty(), "nothing folded");
        assert_eq!(
            coalesced.next_window_end(),
            Some(start + Duration::from_millis(10) + WINDOW),
            "the closed window leaves the open one"
        );
    }

    /// A flush takes every wake still folded, so a test never waits out a
    /// window to see the pass it asked for.
    #[test]
    fn a_flush_takes_every_wake_still_owed() {
        let mut coalesced = Coalesced::new(WINDOW);
        let start = Instant::now();
        coalesced.wake("session", start);
        coalesced.wake("session", start);

        assert_eq!(coalesced.take_all(), vec!["session".to_string()]);
        assert!(
            coalesced.due(start + WINDOW).is_empty(),
            "and the window it was taken from owes nothing more"
        );
        assert!(
            coalesced.wake("session", start + WINDOW),
            "the window closed, so the next wake is at once again"
        );
    }
}
