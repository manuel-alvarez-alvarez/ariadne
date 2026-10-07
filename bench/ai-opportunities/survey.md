# Where Kev could help beyond permissions

This survey maps candidate uses of a Kev-style scored model inside Ariadne,
beyond the one place it already runs: the `ai` permission mode
(`crates/ariadne-daemon/src/ai_permissions/`, `bench/ai-permissions/`). It is
a map, not a recommendation to build: no candidate below is measured, and
none should be read as working until an experiment says so.

## 0. Kev today, as a fact base

- **Model**: Kev wraps a Qwen3.5 base plus a LoRA adapter, in four pinned
  sizes ("flavours"). Pins live in
  `crates/ariadne-daemon/src/ai_permissions/flavours.rs:22-46` (`pins`):
  `kev-0.8b` → `Qwen/Qwen3.5-0.8B-Base`@`dc7cdfe2`; `kev-4b` (the default,
  `bench/ai-permissions/evaluators/kev/__init__.py:21`, `run =
  "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101"`) →
  `Qwen/Qwen3.5-4B-Base`@`1001bb4d`; `kev-9b` → `Qwen/Qwen3.5-9B-Base`@`68c46c4b`;
  `kev-27b` → `Qwen/Qwen3.8-27B`@`1d4bf0f2`. The Kev library itself — the
  upstream package that defines `kev.checkpoint.Checkpoint`, `kev.api.
  SystemOneRequest` and `kev.serve`, every interface claim in this section —
  is pinned at commit `f1535963cea021439370c23127bc970b6788e730`
  (`const KEV_COMMIT`, `crates/ariadne-daemon/src/ai_permissions/install.rs:20`,
  reused at `flavours.rs:13` and `install.rs:370`). Every API shape described
  below (`setup`, `answer`/`probabilities`, `SystemOneRequest`, `probs`) is
  this pinned revision's surface, read directly from
  `bench/ai-permissions/evaluators/kev/__init__.py`, which itself depends on
  `KEV_COMMIT`'s `kev-venv` interpreter (module docstring, line 6) — not a
  description of Kev at an unpinned or later revision.
  Qwen3.5 and Qwen3.8 are Alibaba's Qwen model family; this survey cites the
  pin in the repository, not an upstream model card, as the authority for
  which weights run — no outside capability claim is made for Qwen3.5/3.8
  here.
- **Question shape**: a Kev call answers exactly one `"score"`-type question
  per request, with a short instruction and an ordered list of textual
  criteria levels (`bench/ai-permissions/evaluators/kev/kev_v28.py:13-37`,
  `QUESTIONS = {"decision": {"type": "score", ...}}`). For permissions this
  is three levels (allow/ask/deny text); the harness is general enough for
  any fixed, small number of ordered levels, not for open-ended generation.
  Kev never writes free text and never reasons step by step in the response
  it returns to the caller.
- **Input**: a `state` dict built by `representations.build_normalized`
  from the tool-call request, the workspace path, and `derive(request,
  workspace)` tags (`kev_v28.py:48-49`). The state is a *template over a
  single structured event*, not a transcript, a diff, or a time series.
  Whatever a candidate below wants scored must first be collapsed into a
  comparably small normalized dict.
- **Output**: a probability vector over the criteria levels
  (`model.probs(encoded)`), reduced to one scalar via
  `decision.score_danger` (`kev_v28.py:63-64`), then bucketed by two fixed
  thresholds — `ALLOW_THRESHOLD = 0.0201`, `DENY_THRESHOLD = 0.6321`
  (`kev_v28.py:33-34`), mirrored in Rust at
  `crates/ariadne-daemon/src/ai_permissions/decide.rs:22-27`. A derived-tag
  cap (`CAPS = ["reviewer_directive"]`, `kev_v28.py:35`) can force an
  `allow` down to `ask` before the model ever answers wrong in the unsafe
  direction. A missing or invalid answer defaults to `ask`
  (`bench/ai-permissions/README.md`, "Evaluators" table).
- **Serving**: production calls go through a local subprocess server
  (`crates/ariadne-daemon/src/ai_permissions/server.rs`, `python.rs`,
  invoking `kev.serve`); the benchmark's `KevEvaluator` loads the same
  checkpoint in-process for offline scoring, "so an answer here and a
  served one agree" (`evaluators/kev/__init__.py:3-5`).
- **Memory**: `flavours.rs:56-72` (`memory_bound_gib`) — 0.8b needs 4 GiB
  (CUDA) / 8 GiB (MLX or CPU); 4b needs 12/24/32 GiB; 9b needs 24/32/64 GiB;
  27b needs 80 GiB (CUDA) or 128 GiB (CPU), and never runs on MLX.
- **Latency**: not numerically documented anywhere found in the repository.
  The only latency-adjacent note is qualitative:
  `flavours.rs:75` (`slow`), "A note only: it blocks nothing. Every flavour
  but 0.8b is slow on the CPU." No p50/p95 figures exist for any flavour on
  any device. **This is a measurement gap, not a known number** — any
  claim about a candidate's suitability for a low-latency path (permission
  decisions, attention ordering) is an assumption until an experiment times
  it.
- **Training/eval data**: `bench/ai-permissions/cases/*.jsonl`, scored by
  `run.py measure`/`select`/`report`
  (`bench/ai-permissions/README.md`, "Layout"). This pipeline produces
  labelled CSVs and a bound-selection procedure — the one reusable piece of
  tooling this survey leans on for "how would we evaluate a new mode."

## How to read each candidate

Each candidate below states: the decision today (rule-based or absent), its
integration point (file + symbol), the input Kev would need, whether the
decision fits Kev's scored-question shape or needs generated/open-ended
text instead, a fallback, and what would have to be measured before trusting
it. "Baseline" means the comparison a Kev-backed version must beat: either
the existing rule, or — where none exists — the current agent-driven
workflow with no automation at all.

---

## 1. Failure diagnosis

**Existing behavior.** `fail_task` is an MCP tool
(`crates/ariadne-cli/src/commands/mcp/tools.rs:603-619`) that an agent calls
itself, carrying only a free-text `reason` ("fail_task needs a reason: it
is all the user is told about the task", `tools.rs:610`). The daemon can
also fail a task without agent input, per
`specs/009-scheduler-attention-and-watchdogs.md` rules 14, 26, 29, 34-35:
a wedged relaunch, an exhausted spawn-retry budget, an agent that "stopped
as soon as it started," or an OS open-file-limit shortage. In every case
the stored "reason" is a hand-written or synthesized string — there is no
`FailureReason` enum (the only matches for that phrase are doc comments in
`crates/ariadne-api/src/tasks.rs:44` and `crates/ariadne-store/src/tasks.rs:887`,
both informal). No exit code, stderr capture, or tool-call transcript is
surfaced to the state machine as a structured signal.

**Candidate.** Classify a failed task's cause (environment/setup, agent
misbehavior, flaky test, genuine blocker needing a human) from its final
reason string and/or its last N transcript events, to route it (retry
automatically, surface to a human with a category, or fold into a stats
bucket).

**Fit for Kev's question shape.** Plausible as a scored question ("how
likely is this failure X vs Y vs Z") if the input can be collapsed to a
short templated state the way permission requests are. The free-text
`reason` today is unstructured prose written by another LLM turn, not a
fixed-shape event — it would need a normalization step analogous to
`representations.build_normalized`, which does not exist for failures.

**Proposed integration, concretely.**
- *Normalized input* (new, does not exist today): `{reason: str,
  terminal_status: "failed", seat: Seat, spawn_attempts: u8, open_file_error:
  bool}` — built from the same fields `fail_task`
  (`tools.rs:603-619`) and the daemon's own rule-14/26/29/34 reasons
  already set on the task row; no new data collection, only a new struct.
- *Score output*: one `"score"` question, criteria
  `["environment", "flaky", "agent_error", "needs_human"]`, mirroring the
  three-level shape `kev_v28.py:13-37` already uses, bucketed by two
  thresholds the same way `decide.rs:22-27` buckets danger.
- *Hook point*: **not** `check_transition`
  (`crates/ariadne-core/src/state_machine.rs:155-184`) — that function
  lives in `ariadne-core`, runs before persistence, and takes only `from`,
  `to` and `actor` (`check_transition(from: TaskStatus, to: TaskStatus,
  actor: Actor)`, line 155); it has no `reason`, no `seat`, and no retry
  count, and `ariadne-core` is the crate `ariadne-daemon` depends on, not
  the reverse — wiring Kev in there would make core depend on the daemon's
  AI-permissions machinery. The daemon-owned point that actually has
  `reason` after persistence is the `Change::TaskUpdated { task, transition
  }` arm of `fatten` (`crates/ariadne-daemon/src/bus.rs:298-305`, the
  function starting at `bus.rs:286`),
  fed by `Store::transition_task`
  (`crates/ariadne-store/src/tasks.rs:604-619`), which writes the audit row
  carrying `reason` and publishes this `Change` — every `fail_task` call
  (`tools.rs:603-619`) and every daemon-initiated failure passes through
  `transition_task`, so this is the one place both sources already meet,
  after the fact, with `reason` on hand.
- *Deterministic fallback*: on a missing/invalid Kev answer, label the
  failure `needs_human` — the same fail-safe direction `ai_permissions`
  already takes (a missing answer gives `ask`, `bench/ai-permissions/README.md`)
  — so an unscored failure is never silently retried or silently dropped
  from the attention path.

**Baseline.** Today's "classification" is a human reading the reason
string in the UI or CLI; there is no automated bucketing to beat.

**Data/labels.** `stats::session_fact` records `session_ended` facts
including an attention reason and turn/token counts
(`specs/023-stats.md`, Scope section) — a plausible label source if reasons
were retroactively bucketed by a human once, but no existing labelled
corpus of "why did this task actually fail" was found.

**Latency/memory/frequency.** Off the critical path — failures are rare
relative to tool calls, so this would not compete with the permission
model's 0.8b-sized low-latency slot even if colocated.

**Error consequence.** Misclassifying a failure only affects routing/stats,
not safety — lower stakes than a permission miss, which argues for trying
this candidate before higher-stakes ones.

**What would change the recommendation.** The `failures/` experiment
(`bench/ai-opportunities/failures/report.md`) measuring whether a Kev-style
classifier agrees with human-labelled failure categories at useful
precision, on real `fail_task` reasons and daemon-synthesized reasons
collected from the store.

**Reject/defer reasons.** No structured input exists yet; building the
normalization layer is itself non-trivial work this survey does not
scope. Defer until the experiment shows the free-text reasons carry enough
signal.

---

## 2. Progress detection

**Existing behavior.** There is no semantic progress detector anywhere in
the scheduler. What exists is a pure liveness clock:
`crates/ariadne-daemon/src/scheduler/quiet.rs:62-134` (`check_session_quiet`)
computes `quiet_secs = now - last_heard_from` and acts at three fixed
thresholds — `QUIET_NUDGE_SECS` (180s), `QUIET_FLAG_SECS` (600s),
`QUIET_RELAUNCH_SECS` (1800s), per `specs/009…md` rule 9. Stall tracking
(`crates/ariadne-daemon/src/scheduler/tasks.rs:1108`, `check_stall`) is a
store projection of this same session-level silence flag ("the flag on the
session is the record of it," `tasks.rs:1105-1107`), not a measure of
whether tool calls are converging on anything. Nothing counts repeated
tool calls, diff churn, or looped prompts.

**Candidate.** Score an in-progress turn's recent tool-call/event window
for "looping/stuck" vs "making headway," to nudge or relaunch earlier than
a fixed silence timeout would, or to avoid nudging an agent that is working
hard but briefly silent (e.g., mid-compile).

**Fit for Kev's question shape.** This is the shakiest fit in the set. A
"score" question wants a single templated state; a looping-detector needs a
*window* of events (a sequence), which is a different problem shape than
Kev's one-shot normalized dict. Collapsing a window into one state (e.g.,
"last 5 tool calls were identical") is possible but invents a new
representation with no analogue in `representations.build_normalized`
today.

**Proposed integration, concretely.**
- *Correction*: `check_session_quiet`
  (`crates/ariadne-daemon/src/scheduler/quiet.rs:62-134`) does not read an
  event stream or any tool-call window at all. Its only input is
  `self.last_heard_from(session)` (`quiet.rs:156-169`), which takes the max
  of two row timestamps, `session.last_activity_at` and
  `session.launched_at`, parsed with `chrono::DateTime::parse_from_rfc3339`
  — a single clock reading, not a sequence. The actual reader of a
  session's tool-call/event history is `Store::list_events`
  (`crates/ariadne-store/src/events.rs:186`, over `EventFilter`, returning
  `Vec<AgentEvent>` as defined at `crates/ariadne-store/src/entities.rs:559`)
  — a function `check_session_quiet` never calls today.
- *Normalized input* (new): a fixed-width window, e.g. `{last_5_events:
  [str; 5], distinct_count: u8, quiet_secs: u64}`, built by a new call to
  `Store::list_events` (`events.rs:186`) filtered to the session, taken
  alongside `last_heard_from`'s clock reading — no such window
  representation exists yet; this is new, not a reuse of
  `representations.build_normalized`.
- *Score output*: a `"score"` question with criteria
  `["working", "unclear", "looping"]`, bucketed the same way as `decide.rs`.
- *Hook point*: inside `check_session_quiet`
  (`quiet.rs:62-134`), as an additional check — reading `list_events`
  rather than only `last_heard_from` — gating the nudge/flag/relaunch
  decision that today fires purely on `quiet_secs` against
  `QUIET_NUDGE_SECS`/`QUIET_FLAG_SECS`/`QUIET_RELAUNCH_SECS`.
- *Deterministic fallback*: on a missing/invalid answer, or if `quiet_secs`
  has not yet crossed `QUIET_NUDGE_SECS`, keep the existing fixed-threshold
  rule unchanged — the model can only narrow the window in which a nudge
  fires, never replace the thresholds outright, so a Kev outage degrades to
  exactly today's behavior.

**Baseline.** The three fixed quiet thresholds in `quiet.rs`. Any model
here must beat false-nudge / false-relaunch rates against those constants,
which are simple, well-understood, and already tuned against production
use (spec 009 rule 9-14).

**Data/labels.** No existing labelled corpus of "was this agent actually
stuck" distinct from "was it silent" — these are conflated today, so even
collecting ground truth requires new instrumentation.

**Latency/memory/frequency.** Would need to run far more often than a
permission check (every quiet-check sweep tick, not every tool call), which
raises the colocation question in §10 differently: it is lower-frequency
per session than tool calls, but the sweep itself runs continuously across
all sessions.

**Error consequence.** A false "stuck" verdict triggers a premature
relaunch, destroying in-progress agent state; a false "fine" verdict delays
a human from an actually-wedged agent. Both directions are costly — higher
stakes than failure diagnosis.

**What would change the recommendation.** The `progress/` experiment
(`bench/ai-opportunities/progress/report.md`): does any windowed
representation of the event stream let Kev (or a classifier of similar
size) distinguish stuck from slow-but-working sessions better than the
fixed-threshold rule, on real quiet/stall incidents from the store.

**Reject/defer reasons.** No representation exists today for a window of
events; building and validating one is a prerequisite experiment in itself,
separate from scoring it. This is the candidate this survey is least
confident belongs in Kev's question shape at all — it may need a different
kind of model (sequence-aware) rather than a reused scored-question Kev
mode.

---

## 3. Handoff selection

**Existing behavior.** "Handoff" names two distinct things in the specs,
neither of which is a learned/scored choice today:

- *Outside-session resume* (`specs/020-session-adoption.md`): a human
  explicitly resumes an outside ACP conversation as an Ariadne session via
  `POST /v1/outside-sessions/resume` with `{agent_id,
  internal_session_id}` (rule 8), discovered through `session/list` (rule
  1). This is user-driven adoption, not Ariadne choosing a target.
- *Automatic model/agent switch on exhaustion* (`specs/009…md` rules
  36-40): "A pass switches an ended exhausted session when its work
  remains active" (rule 36); "Candidate order is another agent at the same
  rank, the same agent at that rank, then those steps one rank above, then
  one rank below… Catalog order breaks ties. The ladder is `fast`,
  `balanced`, `frontier`; `local` is never selected" (rules 37-39). This is
  a static, hand-ordered ladder triggered reactively by a provider
  exhaustion error, not a prediction of which agent/model is likeliest to
  finish the task.

**Candidate.** Replace or augment the fixed exhaustion ladder with a score
of "how likely is candidate agent/model X to finish this task from here,"
using the task's recent state as input.

**Fit for Kev's question shape.** Fits reasonably well as a scored
question over a small fixed candidate set (the ladder already enumerates a
bounded list per rule 37) — closer to the permission shape than progress
detection is, since the decision is still a one-shot classification over a
small ordered/categorical set, not a sequence judgment.

**Proposed integration, concretely.**
- *Normalized input*: `{task_title: str, task_description: str,
  candidates: [{agent_id, model, rank}], attempts_so_far: u8}` — the
  candidate list is exactly what `Scheduler::auto_switch_exhausted`
  (`crates/ariadne-daemon/src/scheduler/auto_switch.rs:40`, called from
  `mod.rs:482` and from within `auto_switch.rs:36`) already enumerates
  before applying the static ladder order (rules 37-39); no new discovery
  logic, only a new scoring step over an existing list.
- *Score output*: a `"score"` question ranking each candidate against
  criteria such as `["likely to finish", "likely to need another switch",
  "likely to fail outright"]`, reduced to a per-candidate scalar the way
  `score_danger` reduces Kev's probability vector (`kev_v28.py:63-64`).
- *Hook point*: inside `Scheduler::auto_switch_exhausted`
  (`auto_switch.rs:40`), which calls `self.switch_target(session, &used)`
  to pick the ladder's next model — a scoring step would reorder the
  candidates `switch_target` chooses among, rather than replacing rule
  37-39's eligibility filter (enabled, not exhausted, not `local`).
- *Deterministic fallback*: on a missing/invalid score for any candidate,
  `auto_switch_exhausted` keeps the existing catalog-order ladder
  unmodified (rules 37-39) — the function's existing tie-break, "catalog
  order breaks ties" (rule 38), already defines the no-score-available
  path.

**Baseline.** The static ladder (rules 37-39) — simple, deterministic, and
already shipped; a learned ranking must demonstrably reduce failed
hand-offs or wasted turns to be worth the added machinery and risk.

**Data/labels.** `stats` records a `switch` fact kind (`specs/023…md`,
Scope section) — the direct label source: did a given switch lead to a
successful landing or another exhaustion/failure down the line.

**Latency/memory/frequency.** Rare — triggered only on provider exhaustion,
so latency budget is generous; this is one of the few candidates where a
larger Kev flavour (9b/27b) could be justified without touching the
permission path's budget.

**Error consequence.** A bad switch wastes a relaunch and possibly the
task's spawn-retry budget (bounded at 3, rule 14) — moderate cost, bounded
by the existing retry cap regardless of which agent picks.

**What would change the recommendation.** The `handoff/` experiment
(`bench/ai-opportunities/handoff/report.md`): whether a scored ranking of
candidate agents, trained/evaluated against `switch` facts and their
downstream task outcomes, beats the static ladder's success rate.

**Reject/defer reasons.** The ladder is simple and already encodes a
reasonable prior (rank, then agent identity); replacing it needs enough
`switch`-outcome data to justify the complexity, and that data volume is
unverified here.

---

## 4. Model selection

**Existing behavior.** Per `specs/011-models-effort-and-pins.md`, model
choice is a static pin, not a per-turn decision: "A goal carries the
orchestrator's pin. Every other pin sits on the agent the task staffs
(017), one per author and per reviewer" (rule 10); "A session freezes its
pin at its first launch: a re-pin steers the next spawn, never the
conversation already running" (rule 11). Pins are stored in
`crates/ariadne-store/src/task_agents.rs` and `models.rs`, exposed over
HTTP in `crates/ariadne-daemon/src/http/pins.rs`. Rank, the one piece of
metadata that *is* used automatically, only feeds the exhaustion ladder in
§3: "Rank does not change how the orchestrator chooses models" (rule 17).
There is no dynamic, per-turn model selection anywhere; the only
automated model movement is the reactive exhaustion switch (§3), not a
predictive choice made before a turn starts.

**Candidate.** Predict, before a task's first launch, which pinned
model/effort is likely sufficient for it (e.g. picking `fast` over
`frontier` for a small, low-risk task), as a *suggestion* a human still
confirms — not a silent override of a user-set pin.

**Fit for Kev's question shape.** Fits the scored-question shape if the
"task" can be summarized into a short state (title, description, affected
area) the way a tool call is normalized today — plausible, but no such
task-level normalization exists; it would have to be built new, unlike
permissions where `representations.build_normalized` already exists.

**Proposed integration, concretely.**
- *Normalized input* (new): `{task_title: str, task_description: str,
  candidate_models: [{id, rank}]}` — no task-level normalization exists
  today; this reuses the same pin list `crates/ariadne-store/src/task_agents.rs`
  already stores, not a new discovery step.
- *Score output*: a `"score"` question per candidate model, criteria
  `["underpowered", "sufficient", "overpowered"]`.
- *Hook point*: the HTTP pin-setting path in
  `crates/ariadne-daemon/src/http/pins.rs`, surfaced as a suggestion
  alongside the existing pin UI, before the task's first launch freezes it
  (spec 011 rule 11) — never after, since a re-pin only steers the next
  spawn.
- *Deterministic fallback*: on a missing/invalid score, show no suggestion
  and leave the user's pin exactly as set — this candidate never
  auto-applies a choice, only annotates one, so a Kev outage is invisible.

**Baseline.** The user's own static pin choice at planning time (spec 003,
011) — the thing being second-guessed is a human decision already made
deliberately, which raises the bar for benefit versus a wrong, intrusive
suggestion.

**Data/labels.** No existing corpus of "this task succeeded at this
model/effort but would have succeeded cheaper" — would require correlating
`task_ended` and `verdict` facts (`specs/023…md`) with pin history, not
collected as a single label today.

**Latency/memory/frequency.** Once per task at most (planning time), so
latency is not a real constraint; frequency is low across the system.

**Error consequence.** A wrong suggestion that's merely declined costs
nothing; a wrong suggestion that's accepted and under-powers a hard task
costs a wasted task cycle and possibly a goal delay — moderate, bounded by
human confirmation if the UI requires it.

**Why this is weak relative to others.** This is explicitly reasoning
about "will this model succeed at this task" from a short description —
closer to judgment than to classification, and the repository offers no
evidence Kev's architecture (a scored multiple-choice question over a
templated single-event state) was built for or tested on this kind of
open-ended competence prediction. This candidate leans toward "requires
extended reasoning," not a scored classification — see §8.

**What would change the recommendation.** No experiment is scoped for this
candidate in this hand-off set (only failure, progress and handoff have
experiments). Absent that, this stays a deferred idea pending a
future-scoped experiment, not a recommendation.

**Reject/defer reasons.** No task-level state normalization exists; the
decision resembles competence prediction from a task description more than
a scored classification; and it second-guesses a human's explicit choice,
which raises the UX and trust bar considerably. Defer.

---

## 5. Review triage

**Existing behavior.** Per `specs/004-authoring-and-review.md`, reviewers
are statically staffed per task (rule 1; rule 6, "Each reviewer the task
staffs (017) gets one session for the whole task"), not dynamically picked
or prioritized by risk. `request_review`
(`crates/ariadne-cli/src/commands/mcp/tools.rs:590-598`) moves a task to
`TaskStatus::UnderReview`. Settlement is unanimous-approval, not weighted:
"any request for changes moves the task to `changes_requested`, whatever
else the review holds. Otherwise the approvals are counted and the task is
`approved` once every reviewer staffed on it has approved" (rule 9). The
only ordering rule found is FIFO: "A reviewer works one review at a time,
oldest first by author order" (rule 13) — not a severity or risk-based
queue. The whole test suite runs once per verdict regardless of diff size
(rule 7). The recent commit `fix(store): count requested changes in the
reviewer stats` (see git log) added a *statistic* about this process, not
a change to triage logic — reviewer-request-change counts are now tracked
but not yet used to prioritize anything.

**Candidate.** Score a pending review's diff for risk (likely to need
changes, likely to touch something sensitive) to order the review queue,
or to decide whether a lighter or stricter review pass is warranted —
analogous to how `kev_v28`'s criteria already describe risky vs. routine
*tool calls*; the same vocabulary could plausibly extend to describing a
*diff's* risk.

**Fit for Kev's question shape.** Reasonable fit for triage-as-classification
(where should this review sit in the queue: routine / needs-care /
high-risk) — a bounded, ordered set of levels, much like the existing
allow/ask/deny levels. Poor fit for *performing* the review itself (finding
the bug, writing the comment), which is generated-text work (see §8) that
existing specs already delegate to full reviewer agents (rule 1), not to a
scored classifier.

**Proposed integration, concretely.**
- *Normalized input*: `{files_changed: [str], lines_changed: u32,
  touches_ci_or_config: bool, reviewer_request_text: str}` — derived from
  the diff `request_review` (`tools.rs:590-598`) already attaches to the
  task, templated the way `representations.build_normalized` templates a
  tool call; no such diff-level template exists yet.
- *Score output*: a `"score"` question, criteria
  `["routine", "needs_care", "high_risk"]`.
- *Hook point*: `Scheduler::rouse_reviewer_for`
  (`crates/ariadne-daemon/src/scheduler/tasks.rs:817-891`), which wakes one
  reviewer for one open review request today in whatever order the
  scheduler's task sweep reaches tasks (rule 13's FIFO-by-author-order); a
  score would reorder which open review this function is called for next,
  not change `review_request_to` (`tasks.rs:907`) or the unanimous-approval
  count rule 9 already reads.
- *Deterministic fallback*: on a missing/invalid score, keep rule 13's
  FIFO-by-author-order queue unchanged — risk scoring only ever reorders
  within that queue, it never skips or auto-approves a review.

**Baseline.** The current FIFO-by-author-order queue (rule 13) and the
unanimous-approval gate (rule 9) — both of which treat every review
identically regardless of estimated risk.

**Data/labels.** The `verdict` stats fact kind and the reviewer
requested-changes counts (`specs/023…md`; the recent store fix) are a
direct, already-collected label source: whether a review of a given shape
historically led to approval or changes-requested.

**Latency/memory/frequency.** Once per `request_review` call — far less
frequent than permission checks, and off any interactive hot path, so a
larger/slower flavour would not visibly cost a user time.

**Error consequence.** Misordering the queue delays attention, it does not
change a verdict — low severity, since the unanimous-approval gate (rule 9)
still catches a bad change regardless of queue order.

**What would change the recommendation.** No experiment in this hand-off
set directly targets review triage (only failure, progress, handoff are
scoped); this candidate would need its own experiment correlating a
Kev-scored diff-risk signal against the existing `verdict`/requested-changes
stats before any recommendation beyond "promising, unmeasured."

**Reject/defer reasons.** None found that would exclude it outright; it is
simply unscoped by the current experiment set — flagged here as a strong
candidate for a future experiment given how directly its label source
(reviewer stats) already exists.

---

## 6. Attention prioritization

**Existing behavior.** `ariadne attention` surfaces sessions where an
`AttentionReason` is set, e.g. `crates/ariadne-daemon/src/scheduler/quiet.rs:119`
setting `AttentionReason::Stalled`. Whether attention is owed at all is a
boolean gate: `work_is_active(store, session)` in
`crates/ariadne-daemon/src/attention.rs:19-55`, keyed off the session's
seat (`Orchestrator`/`Author`/`Reviewer`) and the goal/task status it sits
in — not a severity score. Per `specs/009…md` rule 15, "Attention on a
session means a human must act. It is raised only while the work that
session was started for is still its own to do"; rule 24 says it clears
"by the thing that answers it." No numeric priority field exists on
`AttentionReason`, and the list's effective order comes from the shared
session list ordering — "by last activity, newest first" (spec 020 rule
7, reused generally) — i.e. recency, not urgency or likely impact.

**Candidate.** Score each attention-flagged session for how urgent a
human's response actually is (e.g., "waiting on an irreversible permission
decision" vs. "waiting on a routine approval"), to reorder the attention
list beyond plain recency.

**Fit for Kev's question shape.** Good fit in principle — many
`AttentionReason` variants (`WaitingPermission`, `Stalled`, `AgentError`,
`Exhausted`, …) are already a small fixed enum, and the pending request
behind `WaitingPermission` is literally the same normalized state Kev
already scores for the permission decision itself, making this the
cheapest candidate to prototype: it could reuse the *existing* permission
danger score as a proxy for urgency rather than invent a new question.

**Proposed integration, concretely.**
- *Normalized input*: for `WaitingPermission`, the same `state` dict
  `representations.build_normalized` already builds for the pending call
  (`kev_v28.py:48-49`); for other reasons, `{attention_reason:
  AttentionReason, seat: Seat, quiet_secs: u64}`.
- *Score output*: for `WaitingPermission`, reuse the existing danger
  scalar from `decide.rs` directly as the urgency score — no new question.
  For other reasons, a `"score"` question with criteria
  `["routine", "time_sensitive", "urgent"]`.
- *Hook point*: the attention list assembly behind `ariadne attention`,
  which reads `work_is_active`
  (`crates/ariadne-daemon/src/attention.rs:19-55`) and the session list's
  recency order (spec 020 rule 7) — a score would sort that list's output,
  not change which sessions `work_is_active` includes.
- *Deterministic fallback*: on a missing/invalid score, sort by the
  existing "last activity, newest first" rule (spec 020 rule 7) unchanged.

**Baseline.** Plain recency ordering (spec 020 rule 7) — the bar to beat is
low, since recency carries no information about why a session is waiting.

**Data/labels.** No existing corpus of "how long did a human actually take
to resolve attention reason X" to validate that a proposed ordering
improves real response time; `session_ended` facts record attention
reason and duration (`specs/023…md` rule 9) but have not been analyzed for
this purpose here.

**Latency/memory/frequency.** Attention list reads are infrequent (human
polls or push-notified), far looser than the permission path's latency
budget — a strong candidate for sharing one Kev instance with the
permission model without contention (see §10).

**Error consequence.** Misordering costs attention time only, bounded by
the fact that every flagged session still appears on the list — low
severity, no safety impact.

**What would change the recommendation.** The `progress/` and `failures/`
experiments' underlying data (stall and failure incidents) plus a
dedicated analysis of `session_ended` durations by `AttentionReason` would
show whether reordering by a Kev-derived urgency score shortens real
human response latency versus recency. No such experiment is currently
scoped in the hand-off set; this is flagged as the lowest-cost, most
promising candidate to prototype next given the direct reuse of the
existing permission score.

**Reject/defer reasons.** None found; limited only by lack of a scoped
experiment and of response-latency ground truth.

---

## 7. Other candidates found in source but out of scope for the four scoped experiments

- **Model-switch target choice** collapses into §3/§4 above; no separate
  candidate beyond what's already described.
- **No second ML/classifier backend exists in production.** A stray route
  name, `/v1/permissions/laya`, appears only in a test
  (`crates/ariadne-daemon/tests/it/ai_permissions.rs:319`) and looks like a
  legacy name for the same permissions endpoint, not a second deployed
  model; `laya` otherwise exists only as an alternative *benchmark*
  evaluator under `bench/ai-permissions/evaluators/laya/`, never wired into
  `crates/`. This confirms AI/ML decision-making in the shipped system is
  confined entirely to `crates/ariadne-daemon/src/ai_permissions/` today —
  every candidate above is a genuinely new integration point, not an
  extension of a second existing one.

---

## 8. Classification/scoring vs. generated-text/extended-reasoning work

Kev's one question, fixed-criteria-levels, single-scalar-output shape
(§0) is a good fit only for candidates that reduce to picking among a
small, enumerable set of outcomes from a *compact* state:

| Fits scoring (classification) | Needs generated text / extended reasoning |
| --- | --- |
| Failure cause bucketing (§1), *given* a normalized reason | Writing the human-readable diagnosis itself |
| Handoff/switch target ranking (§3), over the existing bounded ladder | Deciding *how* to brief the new agent on resume |
| Attention urgency bucketing (§6), reusing existing permission-style state | Explaining to the human *why* something is urgent |
| Review-diff risk bucketing (§5), as a queue-ordering signal | Actually reviewing the diff and writing review comments (already an agent's job, spec 004 rule 1) |
| — | Progress/stuck detection (§2): the input is a *sequence*, which strains Kev's single-state question shape even before reasoning depth is considered |
| — | Model selection as competence prediction (§4): judging whether a model will succeed at an open-ended task description is closer to reasoning than classification |

The permission decision this survey starts from is itself the strongest
existing proof that the scoring shape works *for a single, well-specified,
frequent event* — not evidence it generalizes to sequences (§2) or
open-ended competence judgments (§4).

## 9. Sharing one model across uses without delaying permission decisions

Permission decisions sit on an interactive path: a tool call blocks on the
`allow`/`ask`/`deny` answer. `crates/ariadne-daemon/src/ai_permissions/server.rs`
serves this through a single persistent `kev.serve` subprocess
(§0, "Serving"), and no documented latency number exists to say how much
headroom that subprocess has (§0, "Latency" — a measurement gap). Given
that gap, this survey can describe the sharing question but not resolve it:

- Any additional use that is **off the interactive path and infrequent**
  (handoff/switch scoring, §3; review-diff triage, §5; attention
  reordering, §6, which is itself read-time not write-time) can plausibly
  queue behind permission calls on the same server process without a user
  noticing, *if* the server processes requests serially and permission
  calls are rare enough to not queue behind a burst of the other kind —
  unverified without load data.
- Any additional use that is **frequent and synchronous with agent turns**
  (progress detection, §2, if it ran every tool call rather than every
  sweep tick) risks contending with permission latency and should not
  share the same process without a separate capacity experiment.
- A larger flavour (9b/27b, §0 memory table) used for a low-frequency
  candidate (handoff, §3) should run as a **separate process** from the
  permission flavour, since switching flavours in one process would
  change the permission model's own latency and defeats the bench's
  "an answer here and a served one agree" invariant
  (`evaluators/kev/__init__.py:3-5`) if the serving path were shared
  across differently-tuned modes.

**What would change this section.** Direct timing of `kev.serve` under
concurrent permission and non-permission load — not scoped in any of the
four hand-off experiments, and a gap this survey flags rather than fills.

## 10. Provisional ranking

Ranked by expected benefit × feasibility × evaluation quality, given what
exists today — provisional, pending the three scoped experiments:

1. **Attention prioritization (§6)** — reuses existing state/score
   machinery most directly, lowest integration cost, low error
   consequence, but unmeasured response-latency benefit.
2. **Handoff/switch target selection (§3)** — bounded candidate set already
   enumerated by the existing ladder, direct label source (`switch` facts),
   moderate error consequence bounded by the existing retry cap.
3. **Review triage/ordering (§5)** — strong existing label source
   (reviewer verdict stats), low error consequence since the
   unanimous-approval gate still catches mistakes, but not scoped by any
   current experiment.
4. **Failure diagnosis (§1)** — plausible but needs a new normalization
   layer for free-text reasons before scoring is even possible; scoped by
   the `failures/` experiment.
5. **Progress detection (§2)** — weakest fit for Kev's single-state
   question shape (needs a sequence representation that doesn't exist
   yet); scoped by the `progress/` experiment, whose results should decide
   whether this belongs to Kev at all or to a different kind of model.
6. **Model selection (§4)** — lowest-confidence fit: closer to competence
   prediction than classification, second-guesses a human's deliberate
   pin, and has no scoped experiment in this hand-off set. Deferred, not
   ranked for near-term work.

This ranking should move once `bench/ai-opportunities/failures/report.md`,
`progress/report.md` and `handoff/report.md` land; the checkpoint task that
consumes this survey combines it with those measured results, not with
this provisional order alone.
