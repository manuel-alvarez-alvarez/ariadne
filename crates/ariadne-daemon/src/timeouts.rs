//! How long the daemon waits on an agent before it stops waiting.
//!
//! A daemon always runs on [`Timeouts::default`]. The values are given to
//! the runtime and the registry rather than written into them as constants
//! so that a test about one of them running out can shorten it, instead of
//! sitting out the real one.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// Fallback interval for repositories without a working webhook.
    pub forge_poll: Duration,
    /// How long one tunnel connection attempt may take, and how long an up
    /// tunnel may hold no open connection before it counts as down (027).
    pub tunnel_connect: Duration,
    /// The first wait before the tunnel reconnects. Each failed attempt
    /// doubles it, up to 60 seconds.
    pub tunnel_retry: Duration,
    /// How long one detail fetch of a pull request may take: its comments,
    /// checks and base, read off the forge after a repository fetch (026).
    pub forge_details: Duration,
    /// How long a repository's poll worker pauses once a fetch answers that
    /// the forge's rate limit was hit, where the forge's own reset time
    /// could not be read (`ForgeClient::rate_limit_reset`).
    pub forge_rate_limit_pause: Duration,
    /// The longest wait between two timer fetches of a repository whose
    /// last fetch failed for a reason other than a rate limit. Each failure
    /// doubles the wait from `forge_poll`, up to this cap; the first fetch
    /// that works resets it.
    pub forge_poll_backoff_cap: Duration,
    /// How long a repository's poll worker waits after its first wake before
    /// it fetches: a further wake inside that fixed window folds into the
    /// same fetch, so a burst of webhook deliveries for one push costs one
    /// fetch rather than one per delivery.
    pub forge_wake_debounce: Duration,
    /// How long a killed agent's running turn has to end once it is
    /// cancelled.
    ///
    /// The prompt response a cancel draws is the only report of what that
    /// turn spent: an agent killed mid-turn — a relaunch that hands a column's
    /// agent its return, a cleanup after the last `complete_step` — otherwise takes the turn's
    /// tokens with it. An adapter that honours `session/cancel` answers
    /// within a second; one that does not is killed when this runs out, as it
    /// was before.
    pub cancel_grace: Duration,
    /// How long one discovery ACP exchange and its OpenCode delete may each
    /// take, and how long one agent's `session/list` pages may take together.
    pub probe: Duration,
    /// How long an outside conversation can take to load before resume fails.
    pub session_load: Duration,
    /// How long a registry download may take, including its response body.
    pub registry_download: Duration,
    /// How long the model's HTTP server may take to load its weights and answer.
    pub ai_permissions_serve_start: Duration,
    /// How long the server supervisor waits before its first restart.
    pub ai_permissions_serve_restart: Duration,
    /// How long one permission decision may take at the model's local HTTP seam.
    pub ai_permissions_decision: Duration,
    /// How long one advisory failure-diagnosis request may take at the AI
    /// permission model's local HTTP seam (024). The request is bounded by
    /// this alone: nothing an ACP driver or the scheduler waits for ever
    /// waits on it.
    pub failure_diagnosis_decision: Duration,
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
    /// How long a test may wait for one dead agent launch to be noticed
    /// and retried (`crate::scheduler`'s spawn-retry budget). Not a pace
    /// the runtime itself keeps — a dead launch is noticed on the next
    /// [`Self::full_reconcile`] tick, whatever that is set to — but a real
    /// subprocess spawn and exit, proved under the full suite's own load
    /// on every core at once, taking longer than a short tick alone would
    /// suggest. A test driving that budget out sets this rather than
    /// keeping its own `Duration` constant, so every clock a test waits on
    /// stays in one place.
    pub dead_launch_patience: Duration,
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
            ai_permissions_decision: Duration::from_secs(5),
            failure_diagnosis_decision: Duration::from_secs(5),
            transcript_poll: Duration::from_secs(15),
            session_wake: Duration::from_millis(250),
            full_reconcile: Duration::from_secs(5),
            forge_poll: Duration::from_secs(300),
            tunnel_connect: Duration::from_secs(15),
            tunnel_retry: Duration::from_secs(1),
            forge_details: Duration::from_secs(30),
            forge_rate_limit_pause: Duration::from_secs(900),
            forge_poll_backoff_cap: Duration::from_secs(3600),
            forge_wake_debounce: Duration::from_secs(5),
            checkpoint: Duration::from_secs(30),
            dead_launch_patience: Duration::from_secs(30),
        }
    }
}
