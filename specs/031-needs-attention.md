---
id: needs-attention
status: current
updated: 2026-10-10
areas: [api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/attention.rs
  - crates/ariadne-daemon/src/attention/recovery.rs
  - crates/ariadne-daemon/src/attention/mod.rs
  - crates/ariadne-daemon/src/scheduler/auto_switch.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/src/commands/attention/board.rs
  - ui/src/features/goals/attention.test.tsx
  - ui/src/features/goals/attention-alerts.test.tsx
---

# Needs attention

One list of what a person has something to do about right now, read off the
daemon rather than inferred by a client from a task's status or a session's
bare flag.

## Scope

In: the shared item shape, `GET /v1/attention`, the producer registry
(`crate::attention` in `ariadne-daemon`), and the first complete producer —
recovery — which covers a model's quota, every task the daemon will never
retry on its own, and a forge CLI missing from the daemon's PATH.

Out: the recovery semantics themselves — the auto-switch ladder, the
spawn-retry budget, the watchdog thresholds — which are 009's. An agent's
own request and a pull request's next step, which stay on the question and
pull-request surfaces (009, 026, 029) until a later task gives their own
producers (registered already, empty — rule 10) their eligibility rules.

## Behavior

1. `GET /v1/attention` answers `AttentionListDto { items, complete }`.
   `items` is every item every registered producer currently finds;
   `complete` is false where a producer's read failed, so a partial read
   never answers as if nothing were wrong (a failed producer costs the
   list its own items, not another producer's).
2. An item (`AttentionItemDto`) carries a stable `id`, which producer
   raised it (`producer`: `recovery`, and not yet `agent_request` or
   `pull_request` — rule 10), the shared contract's `reason` — why this
   blocker exists (`access`, `quota`, `configuration`, `resource`, or
   `unknown`) — a one-line `summary`, the one `required_action` that
   clears it, `since` (RFC 3339, first observed), every `affected`
   subject (`goal`, `task`, `session` or `repository`, with an id and a
   label identifying it on its own), and a typed `target` (`console`,
   `task`, `pull_request` or `settings`) a client opens the item onto.
   Opening a target does not resolve the item — only the action it names
   does. `producer` and `reason` answer different questions: a later
   producer may share none of recovery's reasons, or raise `unknown` for
   one a client still has to tell apart from recovery's own.
3. Items group only on reliable, already-computed evidence of a shared
   cause, never on a guess at one: every session exhausted on the same
   model, every task failed on the exact descriptor-limit words
   (`scheduler::DESCRIPTOR_LIMIT_REASON`), every repository whose forge
   fetch failed naming the same missing CLI. An `unknown` item is never
   grouped with another, unknown or not — there being no reliable evidence
   it shares a cause with anything is exactly why it reads `unknown`. An
   item's id is derived from its shared key, not issued fresh, so the same
   blocker is the same row across a restart and across the same cause
   recurring on a later poll: the quota, resource and configuration ids
   each name the model, the fixed reason, or the forge program they stand
   for, and nothing a process keeps only in memory.
4. The recovery producer (`crate::attention::recovery`) raises its items
   from evidence the daemon already keeps for its own retry loops, never
   from a guess:
   - `quota`: a session flagged `exhausted` (009), exited, whose work is
     still active (`crate::attention::work_is_active_checked`), grouped
     with every other one exhausted on the same model. Raised only once
     automatic recovery has spent its options on it —
     `scheduler::auto_switch::recovery_block` answers `None` (no item)
     while a successor session is already running the work, or while a
     candidate model remains to try; it answers `Some` (an item) once
     switching is disabled, the spawn-retry budget is spent, or no
     candidate model is left — and names which, and how many switches it
     already spent, in the item's own summary. This is the same question
     `Scheduler::auto_switch_exhausted` asks before acting, read as a fact
     rather than acted on, so the item and the switch it waits on can
     never disagree about which models are still candidates —
     propagating a store error rather than collapsing it to "no
     candidate", since a producer that cannot tell a genuine exhaustion
     from a store that would not answer must say so in `complete` rather
     than raise a false item. An affected session's own label names the
     task it was running (or `orchestrator`, for one that ran none)
     rather than repeating the model every session in the group already
     shares.
   - every `failed` task: `failed` is terminal — nothing in the scheduler
     starts a failed task's agent again, so reaching it is itself proof
     that whatever automatic recovery and the orchestrator could do, they
     already did (rule 7 covers what this means for a task still
     in-progress). One failed on the daemon's own descriptor-limit words
     is `resource`, grouped into one item naming every such task — one
     machine out of descriptors, not one item per task it happened to
     fail. Every other one is its own `unknown` item, naming that task
     alone and carrying its own ended reason as the summary, rather than
     guessed into a group with no reliable evidence it shares a cause
     with another.
   - `configuration`: every enabled forge integration whose `fetch_error`
     names the fixed words [`crate::forge::Cli::binary`] gives a CLI
     missing from the daemon's PATH, grouped by the CLI name the message
     itself names — the fix is the same install wherever it is missing
     from. Any other fetch error raises nothing: `forge/poll.rs` retries
     it forever with no budget to spend, so there is no evidence here
     that recovery has given up rather than still trying, and raising
     every retryable forge hiccup as a human blocker would be exactly the
     speculative classification this producer does not make. A disabled
     integration's fetch error is nobody's business: turning the
     integration off is itself the fix.
5. `access` has no deterministic evidence yet and is not produced by this
   path; naming one from a forge CLI's own free-text error, or from the
   advisory failure classifier (024), would be exactly the speculative
   classification this list does not raise. It is part of the shared
   vocabulary so a later producer with such evidence can use it without a
   schema change.
6. A client reads this list live by invalidating it on the same SSE events
   that already carry its evidence — `session_updated`, `task_updated`,
   `repository_updated`, `repository_deleted` and `goal_deleted` — rather
   than a dedicated event: the recovery producer's reads are cheap enough
   that a fresh `GET /v1/attention` on each of those is simpler than
   patching a derived list by hand.
7. The CLI (`ariadne attention`) and Ariadne Desktop (the attention strip,
   its badge and its toasts) each still compose their own list from goals,
   tasks and sessions (009's existing contract) for every reason this
   route does not cover (`waiting_permission`, `waiting_input`,
   `waiting_user`, `agent_error`, and — on a session with no task —
   `disconnected`/`stalled`), with the following suppressed so the two
   lists never say the same thing twice, or raise a row while automatic
   recovery is still trying:
   - a session's bare `exhausted` flag never raises a row of its own on
     either client; only a `quota` item does, read with every other
     reason in the same pass rather than used to gate a locally-derived
     row — a session automatic switching is still working on is not on
     the list at all, under any reason;
   - a task's own bare `failed` status never raises a row either: once
     `GET /v1/attention` can see it, its own item — named and actionable
     — is what both clients show instead (`recovery_affected_task_ids` /
     `recoveryAffectedTaskIds`). The local `failed` reason survives only
     as the fallback a recovery read that could not be read in full
     (`complete: false`) leaves standing, so a task's failure is never
     silently dropped just because the producer that would otherwise
     explain it had a hiccup;
   - a task's `stalled` flag never raises a row, on the task or on its
     session: it is raised while automatic recovery (a nudge, then a
     relaunch) is still working the agent, and reporting it immediately
     would be exactly the "automatic recovery still trying" row this
     contract asks never to raise. If the relaunch also fails, the task
     fails, and the point above covers it;
   - a task-tied session's `disconnected` flag never raises a row either,
     for the same reason: the scheduler resumes a disconnected column
     agent automatically, and only a task that goes on to fail — which
     the point above already covers — means that resume, too, ran out of
     tries. A session with no task (an orchestrator's, a pull request's)
     is unaffected: there is no task whose failure stands in for it, so
     `disconnected` and `stalled` are still read locally for those.

   Every recovery item is its own row, with no goal of its own the way a
   composed task or session row has one: the CLI's `RECOVERY` section
   below the per-goal board, and a Desktop strip row synthesized straight
   from the item (`AttentionItem.recovery`), rendering the item's own
   summary, action, since, reason and `affected` list (each entry shown,
   not only the single `target`) rather than a label derived from a bare
   flag.
8. A client never reads a partial or failed read as "nothing needs
   attention": a query failure, or `complete: false` with no items, holds
   the list open rather than letting it read as all-clear. The CLI prints
   a note instead of the empty state under `--format table`, marks
   `complete` in its own `--format json`, and fails the command under
   `-q` — a bare identifier stream has no field to carry `complete` in, so
   an incomplete read says so the only way it can, by exit status, rather
   than succeeding silently on whatever ids it did get. Desktop folds
   `complete: false` into the same `error`/`partial` path a query failure
   already takes (one code path, not two to keep in step), and the
   shell's window title and sidebar badge — not only the strip — read
   `(!)`/`!` rather than the quiet state while zero items sit under an
   unfinished read.
9. A store error the recovery producer's own evidence depends on
   propagates into `complete: false` rather than being read as "nothing
   found": `work_is_active_checked` and `scheduler::auto_switch`'s
   `switch_chain`/`switch_target` each return a `Result`, distinct from
   the bool-collapsing `work_is_active` the scheduler's own sweeps use
   (009 rule 26 — a sweep is right to wait a failed read out for the next
   tick, since nothing else is owed on a miss; a producer answering one
   HTTP read is not owed a next tick, and must say so instead). The one
   exception is a pull request genuinely gone (`StoreError::NotFound`):
   that is `work_is_active_checked`'s own legitimate "not active" per 029,
   not a read that failed, and does not propagate.
10. Registering a further producer is adding its own module beside
    `recovery` (`crate::attention::<name>`) and a call to it from
    `crate::attention::collect`; nothing about the route or the DTO
    changes to add one. `agent_requests` and `pull_requests` are
    registered already, each answering an empty list, so the route a
    later task's producer and client migration need already compiles and
    runs; the question and pull-request eligibility rules themselves, and
    migrating the client behavior they already carry onto this route, are
    out of this spec's scope (see Scope).

## Acceptance criteria

- The route answers an empty, complete list when nothing is stuck, and is
  in the API document under the `attention` tag
  (`attention.rs::an_empty_daemon_answers_an_empty_complete_list`).
- A task failed on the descriptor-limit words is a `resource` item naming
  that task (`attention.rs::a_descriptor_limit_failure_is_a_resource_item`),
  two tasks sharing the same words are one grouped item
  (`attention.rs::two_tasks_sharing_a_descriptor_limit_failure_are_one_grouped_item`),
  and a task failed for an unrelated reason is its own `unknown` item
  carrying that reason
  (`attention.rs::a_task_failed_for_another_reason_is_its_own_unknown_item`),
  with two such tasks staying two separate items rather than one grouped
  guess
  (`attention.rs::two_tasks_failed_for_different_reasons_stay_separate_unknown_items`).
- An exhausted session on a model the catalog does not rank — so automatic
  switching has nothing to try — is a `quota` item naming that session,
  targeting its console, naming the producer as `recovery`, and naming the
  session's affected entry by its seat rather than the model
  (`attention.rs::an_exhausted_session_on_an_unranked_model_is_a_quota_item`);
  a task-tied session's affected entry instead names its task
  (`attention.rs::a_quota_items_affected_entry_names_the_sessions_task`);
  one already switched to a successor raises nothing
  (`attention.rs::an_exhausted_session_already_switched_raises_no_quota_item`);
  and one nobody is waiting on raises nothing even while exhausted
  (`attention.rs::an_exhausted_session_nobody_is_waiting_on_raises_no_quota_item`).
- A forge integration whose fetch failed on the daemon's own "CLI not
  installed" words is a `configuration` item naming the repository
  (`attention.rs::a_missing_forge_cli_is_a_configuration_item`); one whose
  fetch failed on anything else raises nothing, since the daemon keeps
  retrying it with no evidence recovery has given up
  (`attention.rs::a_forge_fetch_error_that_names_no_missing_cli_raises_nothing`);
  a disabled integration's fetch error raises nothing either
  (`attention.rs::a_disabled_forge_integrations_fetch_error_raises_nothing`).
- The same recovery item answers the same id across two reads, and that id
  is derived from the shared cause alone
  (`attention.rs::a_recovery_items_id_is_stable_across_two_reads`).
- The CLI board raises no `exhausted` or `stalled` row of its own, and a
  task-tied session's `disconnected` only once its own board rule applies
  (`board.rs::an_exhausted_session_raises_no_row_of_its_own_on_this_board`,
  `::a_flagged_session_row_spells_the_reason_the_ui_spells`,
  `::a_row_names_what_it_is_about`); every recovery item, `quota` included,
  prints in its own section with its reason, its waiting time, its action
  and what it affects
  (`attention.rs::every_recovery_item_prints_in_its_own_section`); a task
  any recovery item names is left off the board's own rows, whatever its
  reason
  (`attention.rs::recovery_affected_task_ids_names_every_cause`); `-q`
  names every recovery item's own id alongside the board's
  (`attention.rs::quiet_rows_also_names_every_recovery_item`); and `-q`
  fails the command on an incomplete read rather than succeeding silently
  (`attention.rs::quiet_mode_fails_on_an_incomplete_recovery_read`).
- Ariadne Desktop's attention list raises no row for an exhausted session
  recovery has not given up on, raises one once a `quota` item names it,
  counts a resource or configuration item that names no goal or session,
  does not also count a task's own row once a recovery item already names
  it, raises no row for a task-tied stalled or disconnected session, and
  still raises one for a stalled or disconnected session with no task
  (`ui/src/features/goals/attention.test.tsx`).
- Neither client reads an incomplete or failed read as all-clear: the CLI
  notes it instead of printing the empty state and fails under `-q`, and
  Desktop's title and badge read unavailable rather than quiet while zero
  items sit under an unfinished read
  (`ui/src/features/goals/attention.test.tsx`, "never reads an incomplete
  recovery read as nothing needing attention";
  `attention.rs::quiet_mode_fails_on_an_incomplete_recovery_read`).

## Known gap

- No `access`/credential cause is produced yet: no evidence in the daemon
  today tells a transient forge auth hiccup from one recovery has
  genuinely given up on, and the same reasoning that keeps an unmatched
  forge fetch error from raising an `unknown` item (rule 4) keeps `access`
  unimplemented rather than guessed at. A later producer with reliable
  evidence may use the `access` reason already in the vocabulary.
- `agent_requests` and `pull_requests` are registered producer modules
  that answer empty: the question and pull-request eligibility rules this
  route was built for, and migrating the client behavior that already
  covers them onto it, are a later task's.
- A session stalled or disconnected with no task — an orchestrator's, a
  pull request's — is still read entirely off the client's own composed
  list (rule 7): there is no task whose failure could stand in for one,
  and no "goal failed" status to generalize the task-level rule onto
  without a new threshold this task does not add. `stalled`'s escalation
  past that point is the watchdog thresholds of 009, untouched here.
- A real, end-to-end daemon proof that automatic recovery still in
  progress raises nothing — catching a session mid-switch, with a real
  catalog and a live scheduler, rather than asserting the shared
  `recovery_block`/`switch_target` functions directly — is not included:
  `auto_switch.rs`'s own suite already exercises `switch_target`'s
  candidate selection exhaustively on the live scheduler path, and
  `quota_items` calls the exact same function rather than a copy of its
  logic, which is what keeps the two from disagreeing. A full daemon
  restart, as opposed to the id's derivation from persisted data alone, is
  likewise not exercised: nothing else in this suite restarts the daemon
  binary either, since the integration tests run the router in-process.

## Sources

`crates/ariadne-api/src/attention.rs`, `crates/ariadne-daemon/src/attention/`
(`mod`, `recovery`, `agent_requests`, `pull_requests`),
`crates/ariadne-daemon/src/http/attention.rs`,
`crates/ariadne-daemon/src/scheduler/auto_switch.rs` (`recovery_block`,
`switch_chain`, `switch_target`), `crates/ariadne-cli/src/commands/attention.rs`,
`ui/src/features/goals/attention.ts`, `ui/src/features/goals/attention-alerts.tsx`.
