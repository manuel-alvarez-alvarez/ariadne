---
id: metrics
status: current
updated: 2026-10-21
areas: [daemon, store, docs]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/metrics.rs
  - crates/ariadne-daemon/tests/it/forge_rate_limit.rs
  - crates/ariadne-daemon/tests/it/events.rs
  - crates/ariadne-daemon/tests/it/ai_permissions_decisions.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/src/metrics.rs
---

# Metrics

The metric catalog the daemon records. The daemon records each instrument
through the global OpenTelemetry meter that the telemetry pipeline sets
(033), and the telemetry database keeps it. The metrics are the one source
of stats. The stats ledger of 023 is gone.

## Scope

In: every instrument of the catalog, its type, its unit, its attributes, its
buckets and the place that records it; the rules every attribute obeys; the
removal of the ledger.

Out: how a point is stored, rolled up and retired (033); the Prometheus and
Loki reads and `ariadne query` (034); the shipped dashboard (036).

## Behavior

### Rules for every instrument

1. The daemon is the one recorder. The store records no metric.
2. An attribute never holds a goal id, a task id, a session id, a launch id
   or a path. `repo` is the last part of the repository's checkout path, the
   name the `repo ls` path ends in. `model` is the catalog id,
   `<agent>:<model>`. No instrument has an `agent` attribute.
   The `tool_name` label of tool-call and permission metrics is the stable
   provider tool name where ACP supplies one, else its standard `kind`, else
   `unknown`. It never uses a display title, tool-call id or tool input.
   Those call details remain in the transcript. Existing metric names and
   labels remain unchanged; this rule applies to newly recorded points.
3. An absent value is the label `none`: a session with no seat, a task with
   no column, a flag that is not raised, a goal over several repositories.
4. A counter is monotonic. A histogram is in seconds, with explicit buckets
   fit to its range, which the meter provider's View
   (`metrics::view`, set in `telemetry::init`) gives it: lead times,
   lifetimes and waits on a person from 1 s to 1 day; requests, passes and
   AI decisions from 1 ms to 30 s; launches, turns and forge fetches from
   0.5 s to 1 hour.
5. The Prometheus name of an instrument follows 034 rule 2: `.` becomes `_`,
   a counter ends in `_total`, a unit of `s` adds `_seconds` and a unit of
   `By` adds `_bytes`, and a histogram is `_bucket` (with `le`), `_sum` and
   `_count`.

### Where the work is recorded

6. A task's endings and the time it spends in each status are recorded from
   the store's change hook in the daemon (`bus.rs`), where each
   `TaskUpdated` change carries its transition once. A task has many
   writers, and the hook sees all of them.
7. A goal's ending is recorded at the two calls that end it, cancel and
   complete. Its `GoalUpdated` change carries no transition, so the hook
   cannot tell an ending from a later edit of an ended goal. Both calls end
   it through `Store::end_goal`, which checks and writes in one transaction:
   of two endings racing for one goal, one lands and is recorded, and the
   other is a 409.
8. An attention raise and clear are recorded from the change hook, off each
   session row the store sends after a write of its flag. A flag first seen
   with an `attention_since` from before the daemon started counts no raise.
9. A run's end is recorded from the change hook, off the first committed row
   of a launch that is no longer live. A repeated end, a kill and the end its
   agent reports after it, and every later write of the ended row carry the
   same launch, and count nothing more; the session's next launch ends on its
   own. A row that ended before the daemon started counts nothing.
10. A usage write records the step each counter rose, as
    `Store::upsert_session_usage` answers it from the read and the write of
    one transaction. Two reports racing on one source each count their own
    step.

### Group 1: what the work did

| Instrument | Type | Unit | Attributes | Recorded at |
| --- | --- | --- | --- | --- |
| `ariadne.tasks.ended` | counter | | `status`, `step`, `landed`, `workflow`, `repo`, `model` | change hook, a transition to `finished`, `cancelled` or `failed` |
| `ariadne.task.lead_time` | histogram | s | `status`, `step`, `workflow`, `repo` | the same, from the task's creation to the ending |
| `ariadne.task.status_time` | histogram | s | `status` (the status left) | change hook, each transition, from the transition before it (or the task's creation) |
| `ariadne.goals.ended` | counter | | `status`, `workflow`, `repo` | goal cancel and complete |
| `ariadne.goal.lead_time` | histogram | s | `status`, `workflow` | the same, from the goal's creation |
| `ariadne.sessions.ended` | counter | | `status`, `seat`, `model`, `step`, `attention_reason` | change hook, the first committed row of a launch that is no longer live |
| `ariadne.session.lifetime` | histogram | s | `seat`, `model`, `step` | the same, from the launch (or the creation) to the end |
| `ariadne.tokens` | counter | | `kind` (`input`, `cached_input`, `output`), `model`, `seat`, `repo`, `step` | each usage write, ACP and ingestion, by the delta it adds |
| `ariadne.switches` | counter | | `reason`, `automatic`, `same_agent`, `from_model`, `to_model` | `Launcher::switch_session`, before the pin moves |
| `ariadne.messages` | counter | | `kind`, `from_actor`, `to_actor` | each stored message of the channel |
| `ariadne.permissions` | counter | | `decided_by`, `answer`, `tool_name` | each ACP permission answer, by its stable tool identity |
| `ariadne.permission.wait` | histogram | s | `decided_by` | the same, from the request to the answer |
| `ariadne.attention.raised` | counter | | `reason` | change hook, a flag raised |
| `ariadne.attention.cleared` | counter | | `reason` | change hook, a flag cleared or replaced |
| `ariadne.attention.wait` | histogram | s | `reason` | the same, from `attention_since` to the clear |

11. A usage write records the delta of each kind. A delta below zero records
    nothing, so spend moves while a turn runs and never goes down.
12. `landed` is whether the task carries the commit its change landed as.
    `model` of a task is the model of the agent of its first column with
    `gate: committed` (030), or, where the workflow names no such column, the
    model of the agent of its last column, as before. A task that never
    reached a column names no model either way.
13. `answer` is `allow` where the chosen option is an allowing one or the AI
    model allowed, `cancelled` where no option was chosen, else `deny`.
14. `step` is the workflow column id of an agent session's staffed task
    agent. A session no column staffs — the orchestrator's, a reviewer's, a
    keeper's, or one resumed loose — carries `none`.

### Group 2: live state

| Instrument | Type | Attributes |
| --- | --- | --- |
| `ariadne.tasks.active` | gauge | `status`, each status but the three endings |
| `ariadne.goals.active` | gauge | `status`, each status but `completed` and `cancelled` |
| `ariadne.sessions.live` | gauge | `status`, `seat`, `step` |
| `ariadne.attention.pending` | gauge | `reason` |

15. Each collection reads these counts off the store. A gauge callback runs
    on the metric reader's own thread, so the read runs on a small runtime of
    the metrics module's own (`metrics-collect`), never on a daemon thread a
    flush may be blocking. A read that fails, or takes longer than 2 s,
    observes nothing for that gauge and forgets nothing it saw. An attribute
    set the gauge reported before reports zero only where a successful read
    no longer holds it.

### Group 3: daemon health

| Instrument | Type | Unit | Attributes | Recorded at |
| --- | --- | --- | --- | --- |
| `http.server.request.duration` | histogram | s | `http.request.method`, `http.route`, `http.response.status_code` | a layer on the router that both the socket and the TCP listener serve |
| `ariadne.scheduler.tick.duration` | histogram | s | | each periodic full reconciliation |
| `ariadne.acp.launches` | counter | | `outcome` (`ready`, `failed`, `spawn_failed`) | the ACP runtime's spawn and its `session_start` or `session_end` |
| `ariadne.acp.launch.duration` | histogram | s | `outcome` | the same, from the spawn to the session ready, or to its end before that |
| `ariadne.forge.poll.duration` | histogram | s | `repo`, `outcome` (`ok`, `rate_limited`, `failed`) | each fetch of a repository's requests |
| `ariadne.forge.webhooks` | counter | | `outcome` (`accepted`, `refused`, `failed`) | each webhook delivery |
| `ariadne.events.stream.clients` | gauge | | | the followers of `/v1/events/stream` |
| `ariadne.events.stream.resyncs` | counter | | | each follower of the stream that fell behind |
| `ariadne.db.size` | gauge | By | `file` | the size of `ariadne.db` and `telemetry.db`, read on collection |
| `ariadne.telemetry.deleted` | counter | | `table` | the retention sweep (033), by the rows it deleted |
| `process.cpu.time` | counter | s | | the process CPU clock, read on collection |
| `process.memory.usage` | gauge | By | | the resident memory, read on collection |

16. `http.route` is the matched route template, never the path. The layer
    leaves out `/v1/events/stream`, `/v1/logs/stream`, a session's console
    stream and its terminal WebSocket, and every route under `/otlp/`,
    `/api/v1/` and `/loki/`.

### Group 4: agent activity

| Instrument | Type | Unit | Attributes | Recorded at |
| --- | --- | --- | --- | --- |
| `ariadne.turns` | counter | | `seat`, `model`, `step` | each stored `stop` |
| `ariadne.turn.duration` | histogram | s | `seat`, `model`, `step` | the same, from its `user_prompt_submit` |
| `ariadne.tool_calls` | counter | | `tool_name`, `model` | each stored `pre_tool_use`, by its stable tool identity |
| `ariadne.ai_permissions.decisions` | counter | | `verdict` (`allow`, `deny`, `ask`, `unanswered`) | each decision of the AI permission model (022) |
| `ariadne.ai_permissions.latency` | histogram | s | | the same |

### The ledger

17. Migration `0006_drop_stat_facts.sql` drops `stat_facts` from
    `ariadne.db`. The routes under `/v1/stats/*`, their DTOs, and the store's
    and the daemon's ledger code are gone.

## Acceptance criteria

- A finished task counts one ending with its column, model, workflow,
  repository and landing, one lead time, and each status it left with the
  seconds it spent there; no label holds its id
  (`metrics.rs::a_finished_task_counts_its_ending_and_times_each_status_it_left`).
- A task cancelled before its first column names no column and no model
  (`metrics.rs::a_task_cancelled_before_its_first_column_names_no_column`).
- A finished task names the model of its committed-gate column, even where
  it finishes on a later column on another model
  (`metrics.rs::a_finished_task_names_the_model_of_its_committed_gate_column`).
- A cancelled goal counts one ending and one lead time, and a refused second
  cancel counts nothing (`metrics.rs::a_cancelled_goal_counts_one_ending`).
- A completed goal counts one ending with its repository
  (`metrics.rs::a_completed_goal_counts_one_ending`).
- Of a cancel and a completion racing for one goal, one lands and one ending
  is counted (`metrics.rs::racing_endings_of_one_goal_count_one`).
- Usage writes count their deltas by kind, model, seat and repository, and a
  smaller total counts nothing
  (`metrics.rs::usage_writes_count_their_deltas_and_never_go_down`).
- Reports racing on one source count each step once
  (`metrics.rs::racing_usage_reports_count_each_step_once`), and a usage
  write answers the step it moved
  (`store.rs::a_source_replaces_its_own_totals_and_sources_add_up`).
- A run counts one ending and one lifetime however often its end is
  reported, and its next launch ends on its own
  (`metrics.rs::a_run_counts_one_ending_however_often_it_is_reported`).
- A kill and the end its agent reports are one end
  (`metrics.rs::a_kill_and_the_end_its_agent_reports_count_once`).
- A raised and cleared flag counts the raise, the clear, the wait and the
  pending flag
  (`metrics.rs::a_raised_and_cleared_flag_counts_the_raise_the_clear_and_the_wait`).
- The live-state gauges read the store on collection, a status that empties
  reads zero, and the database size and the process metrics are read
  (`metrics.rs::the_live_state_gauges_read_the_store_on_collection`).
- A session on no column carries no step
  (`metrics.rs::a_session_on_no_column_carries_no_step`).
- A failed gauge read observes nothing, and only a successful read reports
  an emptied attribute set as zero
  (`metrics.rs::a_failed_read_reports_no_zero_and_an_empty_read_does`).
- Each histogram range has the buckets of the View
  (`metrics.rs::each_histogram_range_has_its_own_buckets`).
- A request is timed under its route template, and no label holds its path
  (`metrics.rs::a_request_is_timed_under_its_route_template`).
- A scheduler pass is timed (`metrics.rs::a_scheduler_pass_is_timed`).
- A message is counted by its kind and actors
  (`metrics.rs::a_message_is_counted_by_its_actors`).
- A follower of the event stream is counted while it follows
  (`metrics.rs::an_event_stream_follower_is_counted_while_it_follows`), and a
  lagged one counts a resync
  (`events.rs::sse_stream_signals_resync_and_closes_when_a_client_lags`).
- The retention pass counts what it deletes
  (`metrics.rs::the_retention_pass_counts_what_it_deletes`).
- A refused webhook delivery is counted
  (`metrics.rs::a_refused_webhook_is_counted`).
- Each forge fetch is timed by repository and outcome
  (`forge_rate_limit.rs::each_fetch_is_timed_by_its_repository_and_outcome`).
- Each AI permission decision is counted by verdict and timed
  (`ai_permissions_decisions.rs::a_test_request_scores_the_same_model_state_without_publishing_or_learning`).
- An agent run counts its ready launch and its launch time, its turns and
  their durations, its tool call and console permission answer by stable
  provider name (or category where unnamed), with its permission wait
  (`metrics.rs::an_agent_run_counts_its_launch_turns_tools_and_permission`).
- A requested cross-agent switch counts the models left and entered
  (`metrics.rs::a_requested_switch_counts_the_models_left_and_entered`).

## Sources

- [OpenTelemetry metrics API](https://opentelemetry.io/docs/specs/otel/metrics/api/).
- [OpenTelemetry HTTP semantic conventions](https://opentelemetry.io/docs/specs/semconv/http/http-metrics/).
- [Telemetry](033-telemetry.md) and [Telemetry query APIs](034-telemetry-query.md).
