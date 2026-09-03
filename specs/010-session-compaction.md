---
id: session-compaction
status: current
updated: 2026-09-04
areas: [daemon]
commits: [ab689148]
tests:
  - crates/ariadne-daemon/tests/compaction.rs
  - crates/ariadne-store/tests/store.rs
---

# Session compaction

Why the daemon asks a CLI to compact its own transcript, when it asks, and
what it will not do to a pane while it waits.

## Scope

In: the hand-offs that owe a compaction, how the debt is written and paid,
what protects a compacting pane, when a compaction is written off, and how it
appears in the event log.

Out: the delivery mechanics of typing into a pane (008), and the quiet
watchdog's own timeline (009).

## Behavior

1. Sessions are long-lived and every resume replays the whole transcript as
   its first prompt. Nothing shortens that transcript but the CLI's own
   compaction, and the CLI runs one on its own only near the context limit.
2. So the daemon asks for one at every **hand-off** — the moment the
   conversation so far has served its purpose: a plan finalized, a review
   requested, a verdict given.
3. The debt is written on the session row (`compact_owed_at`) by the
   reconcile pass that sees the hand-off, and one hand-off is paid once
   however many passes see it.
4. Paying it means typing the CLI's own `/compact` into the pane, through the
   same confirmed delivery a nudge takes (008), and only into a pane that is
   free: the turn ended, nothing being typed, no dialog waiting on a person.
   Claude Code, which takes an argument, is given a per-role focus saying what
   to keep.
5. While a compaction is in flight the pane is left alone: nothing is typed
   into it and nothing kills it. A review's feedback, a landing briefing, a
   resume or a nudge that becomes due meanwhile goes out on the pass after.
6. The pane is protected from the moment the command starts going in, not from
   the moment the paste is confirmed — a CLI with little to summarise can
   report the compaction done inside those seconds.
7. The wait ends when the CLI says so in its own vocabulary (Claude Code's
   `SessionStart` from `compact`, Codex's `PostCompact`, OpenCode's
   `session.compacted`), or when the wait runs out. A session is never held
   longer than that, nor for a debt that never got to run
   (`COMPACTION_OWED_FOR_SECS`, 600 s): anything else is written off and the
   work goes on.
8. A session that owes a compaction is not ended until the debt is paid — an
   idle planner past `finalize_plan` and a reviewer that has voted are both
   kept up for it.
9. Each compaction is written to the session's event log as the daemon's own
   `compaction` event, and one that ended any other way as `compaction_failed`
   naming why.
10. A CLI that cannot be told to compact from outside simply is not.

## Acceptance criteria

- Each hand-off owes its session a compaction
  (`compaction.rs::a_review_requested_owes_the_engineer_a_compaction`,
  `::a_verdict_given_owes_the_reviewer_a_compaction`,
  `::a_plan_finalized_owes_the_planner_a_compaction_before_it_is_let_go`), and
  is paid once however many passes see it
  (`::a_hand_off_is_paid_once_however_many_passes_see_it`).
- A session mid-turn or waiting on a person owes the debt but is not typed into
  (`compaction.rs::a_session_mid_turn_owes_the_compaction_and_is_not_typed_into`,
  `::a_session_waiting_on_a_person_is_not_typed_into`).
- A reviewer that voted is ended only once its compaction is done
  (`compaction.rs::a_reviewer_that_voted_is_ended_only_once_its_compaction_is_done`).
- A resume due during a compaction goes out after it
  (`compaction.rs::a_resume_due_during_a_compaction_goes_out_after_it`).
- A compaction reported done before its delivery settles is over
  (`compaction.rs::a_compaction_reported_done_before_its_delivery_settles_is_over`),
  and one nobody reports done is written off
  (`::a_compaction_nobody_reports_done_is_written_off`).
- A cold start over a hand-off keeps the prompt and types nothing
  (`compaction.rs::a_cold_start_over_a_hand_off_keeps_the_prompt_and_types_nothing`).
- The debt round-trips through the store
  (`store.rs::a_session_owes_a_compaction_until_it_is_paid`).

## Sources

`crates/ariadne-daemon/src/scheduler/compaction.rs`,
`crates/ariadne-daemon/src/agents/` (`compaction_done` per CLI),
`crates/ariadne-store/src/sessions.rs`.
