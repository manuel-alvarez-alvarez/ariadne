//! How long a request's head has stood still, for the push a review session
//! is told of (029 rule 8).
//!
//! A push is news to the session only once the head stood unchanged for the
//! whole settle interval, so a run of commits is one round on the last head.
//! A head that differs from the one last seen restarts the interval. The
//! clock is the caller's `now`, so the boundary is proved without waiting.

use std::collections::HashMap;
use std::time::{Duration, Instant};

pub(super) struct HeadSettle {
    /// How long a head must stand still ([`crate::timeouts::Timeouts::review_push_settle`]).
    interval: Duration,
    /// The head each request showed last, and since when, by pull request id.
    seen: HashMap<String, (String, Instant)>,
}

impl HeadSettle {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            interval,
            seen: HashMap::new(),
        }
    }

    /// Whether `head` of request `id` has stood still for the whole interval
    /// at `now`. A head not seen before starts its interval here.
    pub(super) fn settled(&mut self, id: &str, head: &str, now: Instant) -> bool {
        let since = match self.seen.get(id) {
            Some((seen, since)) if seen == head => *since,
            _ => {
                self.seen.insert(id.to_string(), (head.to_string(), now));
                now
            }
        };
        now.duration_since(since) >= self.interval
    }

    pub(super) fn forget(&mut self, id: &str) {
        self.seen.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_secs(60);

    /// 029 rule 8: the head is told only once it stood still for the whole
    /// interval, and not a moment before.
    #[test]
    fn a_head_settles_exactly_when_the_whole_interval_has_passed() {
        let start = Instant::now();
        let mut settle = HeadSettle::new(INTERVAL);
        assert!(
            !settle.settled("pr", "a", start),
            "the first sight starts it"
        );
        assert!(
            !settle.settled("pr", "a", start + INTERVAL - Duration::from_nanos(1)),
            "one instant short of the interval is held"
        );
        assert!(
            settle.settled("pr", "a", start + INTERVAL),
            "the whole interval has passed"
        );
    }

    /// 029 rule 8: a push inside the interval restarts it, so the interval
    /// is counted from the last head and never from the first.
    #[test]
    fn a_new_head_restarts_the_interval() {
        let start = Instant::now();
        let mut settle = HeadSettle::new(INTERVAL);
        settle.settled("pr", "a", start);
        let push = start + INTERVAL - Duration::from_secs(1);
        assert!(!settle.settled("pr", "b", push));
        assert!(
            !settle.settled("pr", "b", start + INTERVAL),
            "the first head's interval is over, the second head's is not"
        );
        assert!(
            !settle.settled("pr", "b", push + INTERVAL - Duration::from_nanos(1)),
            "the second head's interval runs from its own push"
        );
        assert!(settle.settled("pr", "b", push + INTERVAL));
    }

    #[test]
    fn requests_settle_on_their_own() {
        let start = Instant::now();
        let mut settle = HeadSettle::new(INTERVAL);
        settle.settled("one", "a", start);
        assert!(!settle.settled("two", "a", start + INTERVAL));
        settle.forget("one");
        assert!(!settle.settled("one", "a", start + INTERVAL * 2));
    }
}
