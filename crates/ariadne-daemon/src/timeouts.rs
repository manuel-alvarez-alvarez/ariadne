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
    /// How long an outside conversation can take to load before resume fails.
    pub session_load: Duration,
    /// How long a registry download may take, including its response body.
    pub registry_download: Duration,
    /// How long the model's HTTP server may take to load its weights and answer.
    pub ai_permissions_serve_start: Duration,
    /// How long the server supervisor waits before its first restart.
    pub ai_permissions_serve_restart: Duration,
    /// How often the daily model refresh clock checks local time.
    pub ai_permissions_schedule_poll: Duration,
    /// How long one permission decision may take at the model's local HTTP seam.
    pub ai_permissions_decision: Duration,
    /// How often a running turn's transcript is read again for what the
    /// launch has spent, so a long turn's figure moves before it ends.
    pub transcript_poll: Duration,
    /// How long one session's wakes are folded together before the scheduler
    /// reconciles the task behind them again (`crate::scheduler::coalesce`).
    ///
    /// The first wake of a burst is reconciled at once, so this is not a
    /// delay on anything the user waits for: it is the shortest gap between
    /// two reconciles of one session. Short enough that a turn which ends in
    /// the middle of a burst is acted on in a quarter of a second, long
    /// enough that an agent reporting a tool call every few milliseconds does
    /// not read the store out of connections.
    pub session_wake: Duration,
    /// How often the scheduler reconciles everything, whatever it has been
    /// woken about (`crate::scheduler`).
    ///
    /// Not how long a hand-off waits: everything a write can report — a task
    /// moving on, a plan finalized, a verdict, an agent's own event — arrives
    /// as a `SchedEvent` and is acted on as it lands. What is left for the
    /// tick is the state nothing reports, and the one that costs an agent its
    /// turn is an agent that died before it could say so. This period is the
    /// ceiling on how long the successor of such an agent sits unstarted, so
    /// it is short — the pass costs a handful of indexed reads, which is
    /// cheap enough to make five seconds the wait rather than fifteen. A test
    /// about what a wake alone does lengthens it, so the tick cannot do the
    /// work the wake is being watched for.
    pub full_reconcile: Duration,
    /// How often the write-ahead log is folded back into the database
    /// (`crate::checkpoint`). Nothing waits on it: it is the period of a
    /// clock, and a tick that finds readers in the way costs the next one
    /// nothing.
    pub checkpoint: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            cancel_grace: Duration::from_secs(5),
            probe: Duration::from_secs(5),
            session_load: Duration::from_secs(60),
            registry_download: Duration::from_secs(30),
            ai_permissions_serve_start: Duration::from_secs(120),
            ai_permissions_serve_restart: Duration::from_secs(1),
            ai_permissions_schedule_poll: Duration::from_secs(30),
            ai_permissions_decision: Duration::from_secs(5),
            transcript_poll: Duration::from_secs(15),
            session_wake: Duration::from_millis(250),
            full_reconcile: Duration::from_secs(5),
            checkpoint: Duration::from_secs(30),
        }
    }
}
