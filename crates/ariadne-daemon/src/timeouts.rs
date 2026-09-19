//! How long the daemon waits on an agent before it stops waiting.
//!
//! A daemon always runs on [`Timeouts::default`]. The values are given to
//! the runtime and the registry rather than written into them as constants
//! so that a test about one of them running out can shorten it, instead of
//! sitting out the real one.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// How long a killed agent's running turn has to end once it is
    /// cancelled.
    ///
    /// The prompt response a cancel draws is the only report of what that
    /// turn spent: an agent killed mid-turn — a relaunch that hands an author
    /// its review, a cleanup after `finish_task` — otherwise takes the turn's
    /// tokens with it. An adapter that honours `session/cancel` answers
    /// within a second; one that does not is killed when this runs out, as it
    /// was before.
    pub cancel_grace: Duration,
    /// How long one discovery probe of an agent may take, and how long one
    /// agent's `session/list` pages may take together.
    pub probe: Duration,
    /// How often a running turn's transcript is read again for what the
    /// launch has spent, so a long turn's figure moves before it ends.
    pub transcript_poll: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            cancel_grace: Duration::from_secs(5),
            probe: Duration::from_secs(5),
            transcript_poll: Duration::from_secs(15),
        }
    }
}
