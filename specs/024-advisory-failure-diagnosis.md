---
id: advisory-failure-diagnosis
status: current
updated: 2026-10-10
areas: [daemon, api, core, store, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/failure_diagnosis.rs
  - crates/ariadne-daemon/src/failure_diagnosis.rs
  - crates/ariadne-daemon/src/ai_permissions/mod.rs
  - crates/ariadne-daemon/src/http/events.rs
  - crates/ariadne-daemon/src/http/classify.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-api/src/events.rs
  - crates/ariadne-console/src/transcript.rs
  - ui/src/features/sessions/session-activity.test.tsx
---

# Advisory failure diagnosis

An optional, local, second opinion on why an ACP session's turn failed,
shown beside the daemon's own error — never in place of it, and never as a
reason to retry, switch models, or raise attention on its own.

## Scope

In: `ai_failure_diagnosis`, the classifier that reads a failed turn's error
through the AI permission model's already-running local Kev service (022),
the `session.diagnosis` event it may produce, and showing that event's
category beside the `session.error` it is about, in the console, the CLI
and the desktop app.

Out: what makes a prompt exhausted (021, rule 14), the switch a scheduler
starts from it (009), the AI permission model's install, its server, and its
settings (022) — this reads the model's existing live handle and nothing
else about it — and progress detection or any other automatic recovery
policy, none of which this changes or informs.

## Behavior

1. `ai_failure_diagnosis` (`config.toml`, default `false`) turns the
   feature on. Off, nothing in this spec runs: `crate::acp` behaves exactly
   as 021 describes alone.
2. On a failed prompt, once `acp.rs` has built the `session.error` payload —
   `exhausted` and `exhausted_reason` included, exactly as 021 rule 14 sets
   them — and that event is recorded and published, the daemon considers it
   for a diagnosis. The error is never held back for this: nothing about the
   session's own end, or about `exhausted_reason`'s decision, waits on it.
3. A diagnosis is considered only when the model already has a live
   endpoint (022) at that moment. It is never installed, never started, and
   never waited for because an error just happened — a model that is off,
   loading, or absent simply leaves the error to stand alone, as it does
   with the feature off.
4. At most one diagnosis request runs at a time, daemon-wide, claimed with a
   compare-and-swap; a failure considered while one is already running is
   left undiagnosed rather than queued. The request itself is bounded by
   `Timeouts::failure_diagnosis_decision` and carries a bounded snapshot of
   the one error's message and structured data — never a repository file,
   another event, or a transcript.
5. A permission decision needing the same model aborts a diagnosis request
   reaching it first, so the decision an agent's turn is blocked on never
   waits behind an advisory nothing is blocked on. The aborted request frees
   its claim at once, so the next failure is free to be considered.
6. The request classifies the error into one of five categories: `exhausted`
   (quota or rate allocation exhausted), `temporary` (worth retrying),
   `auth_config` (a credential or configuration problem), `task_error` (the
   task or its input failed), or `insufficient` (no reliable evidence
   either way). An answer with no model identity, or an unknown category, is
   not a diagnosis at all. Where the category and the model identity
   validate but the probabilities do not — missing one of the five, out of
   `[0, 1]`, or not finite — the category and model identity are kept and
   the probabilities are not.
7. A diagnosis that validates is recorded as a `session.diagnosis` event,
   through the same ingestion every agent event takes (012): the session id,
   the launch id of the failure it is about, the `error_event_id` of its
   `session.error`, the `category`, the model identity the response itself
   names — never the one the request asked for — and the probabilities
   where they validated. It never moves the session's status,
   raises or clears attention, or counts as activity — read well after the
   failure, it is not a word from the agent. A `session.diagnosis` from a
   launch a session has moved past is still recorded, exactly as any other
   event is (012, rule 12), but changes nothing either way.
8. A `session.diagnosis` is shown beside the `session.error` it names, as an
   advisory: `ariadne events`, `ariadne session logs`/`ariadne task logs`,
   the console, and the desktop app's session activity all read its
   category as one line under that error's own, labeled as the model's
   suggestion (`AI suggests: …`) and never folded into the error's own text.
   It is never its own block: a build that cannot find the error it names —
   the block has already scrolled out of what it is folding — attaches it
   to nothing rather than inventing one.
9. The daemon shuts down once any diagnosis request in flight is cancelled;
   none outlives the process that started it.

## Acceptance criteria

- Disabled, absent, and malformed outcomes all leave the original error
  exactly as 021 alone would
  (`failure_diagnosis.rs::disabled_by_default_produces_no_diagnosis`,
  `::an_absent_model_produces_no_diagnosis_the_same_as_one_still_loading`,
  `::a_malformed_answer_produces_no_diagnosis`), and so does one that times
  out (`::a_diagnosis_that_runs_past_its_bound_times_out_and_produces_none`).
- The original error and the session's end do not wait on a diagnosis, and a
  busy classifier leaves a concurrent failure undiagnosed rather than queued
  (`failure_diagnosis.rs::a_failed_sessions_diagnosis_is_correlated_with_its_own_error`,
  `::a_second_failure_while_one_is_in_flight_is_skipped_not_queued`).
- A permission decision preempts a diagnosis reaching the same model and is
  not delayed by it
  (`failure_diagnosis.rs::a_permission_decision_preempts_a_hanging_diagnosis_and_is_not_delayed_by_it`).
- Kev's own opinion never starts or prevents what `exhausted_reason` and the
  scheduler already decided
  (`failure_diagnosis.rs::model_disagreement_never_moves_the_recorded_exhaustion`).
- The stored model identity is the response's own, not the request's, and an
  answer naming none is not a diagnosis
  (`failure_diagnosis.rs::the_stored_model_identity_is_the_responses_own`,
  `::a_missing_or_empty_model_identity_is_not_a_diagnosis`).
- A stored diagnosis identifies the correct error, by id, including a
  replaced launch, after a restart, and across duplicate or late-arriving
  events
  (`failure_diagnosis.rs::a_failed_sessions_diagnosis_is_correlated_with_its_own_error`,
  `::a_diagnosis_for_a_replaced_launch_keeps_that_launchs_id_and_disturbs_nothing_current`,
  `::the_stored_correlation_survives_a_restart`,
  `transcript.rs::duplicate_errors_each_keep_their_own_diagnosis`).
- A late diagnosis is visible in the console and the desktop app beside the
  original failure, even once a session's own terminal, turn, or process has
  ended (`transcript.rs::a_late_diagnosis_attaches_to_the_error_it_names`,
  `session-activity.test.tsx::attaches a late diagnosis to its original
  error and keeps both once the session has ended`).
- A diagnosis moves neither the status nor the attention a `session.error`
  or any other event would (`classify.rs::a_diagnosis_event_moves_neither_status_nor_attention`).
- A diagnosis in flight is cancelled at shutdown rather than outliving it
  (`failure_diagnosis.rs::shutdown_cancels_a_diagnosis_in_flight`).
- `ai_failure_diagnosis` is read strictly and defaults to off
  (`config.rs::ai_failure_diagnosis_is_read_and_off_by_default`).

## Known gap

The five categories are measured only on a synthetic corpus
(`bench/ai-opportunities/failures/report.md`): real-shaped but not
production data, and no claim here is made about production accuracy or
latency. That is why this spec stops at showing the category, and nothing
in the daemon acts on it.

## Sources

`crates/ariadne-daemon/src/failure_diagnosis.rs` (the classifier),
`crates/ariadne-daemon/src/acp.rs` (where a failure is considered, and a
permission decision preempts one),
`crates/ariadne-daemon/src/ai_permissions/mod.rs` (the preemption handle),
`crates/ariadne-daemon/src/http/events.rs` and `http/classify.rs` (ingestion
and its summary), `crates/ariadne-api/src/events.rs` (the advisory note),
`crates/ariadne-console/src/transcript.rs` (where it attaches to its
error), `crates/ariadne-client/src/endpoint.rs` and
`crates/ariadne-daemon/src/config.rs` (the setting),
`bench/ai-opportunities/failures/report.md` (the evidence and its limits).
