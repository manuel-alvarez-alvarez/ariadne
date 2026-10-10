---
id: needs-attention
status: current
updated: 2026-10-10
areas: [api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/attention.rs
  - crates/ariadne-daemon/tests/it/scheduler_attention.rs
  - crates/ariadne-daemon/src/attention/recovery.rs
  - crates/ariadne-daemon/src/attention/mod.rs
  - crates/ariadne-daemon/src/scheduler/auto_switch.rs
  - crates/ariadne-daemon/src/scheduler/goals.rs
  - crates/ariadne-daemon/src/scheduler/pull_requests.rs
  - crates/ariadne-daemon/src/forge/mod.rs
  - crates/ariadne-daemon/src/forge/poll.rs
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
     with every other one exhausted on the same model *and* blocked for
     the same reason — the grouping key is `(model, reason)`, not the
     model alone, so two sessions on one model blocked for two different
     reasons never collapse into an item that could only report one of
     them. Raised only once automatic recovery has spent its options on
     it — `scheduler::auto_switch::recovery_block` answers `None` (no
     item) while a successor session is already running the work, or
     while a candidate model remains to try; it answers `Some` (an item)
     once switching is disabled, the spawn-retry budget is spent, or no
     candidate model is left — naming which, in the item's own summary
     and required action. This is the same question
     `Scheduler::auto_switch_exhausted` asks before acting, read as a fact
     rather than acted on, so the item and the switch it waits on can
     never disagree about which models are still candidates —
     propagating a store error rather than collapsing it to "no
     candidate", since a producer that cannot tell a genuine exhaustion
     from a store that would not answer must say so in `complete` rather
     than raise a false item. Every affected session's own label names
     the task it was running (or its seat — `orchestrator`, `reviewer` —
     for one that ran none) and its own switch count, rather than
     repeating the model every session in the group already shares, and
     each still carries its own id: a client lists every one of them,
     with its own link to its own console, rather than only the item's
     single `target`, which can open only one of several.
   - every `failed` task whose goal's orchestrator has already had its
     say: `failed` is terminal — nothing in the scheduler starts a failed
     task's agent again — but the orchestrator is told of the failure
     (009 rule 4) and may retry it itself (`http::tasks::retry`), which
     moves the task off `failed`, and so off this list, before a human
     ever needs to. `orchestrator_has_answered_for` reads whether that has
     already happened from evidence persisted for exactly *this* task's
     id, never from a timestamp or a session's ambient `last_activity_at`:
     `tell_orchestrator` records the failed task ids named in the prompt
     it queues (`Goal::orchestrator_told_failed_task_ids`), and the agent-
     event ingest promotes them into `orchestrator_answered_failed_task_ids`
     the moment that orchestrator's session next reports `stop` — the
     turn that actually carried them, not merely the queuing of a prompt
     that turn has not run yet. The gate answers `true` once this task's
     id is in the confirmed list. A session's own activity is never read
     for this: ending a turn already running when the task failed, or one
     confirming an unrelated failure, would move a timestamp regardless of
     whether that turn ever carried *this* task's news, and the confirmed
     list's own membership is immune to both. A goal not `orchestrated`,
     or not `planning` or `active` any more, answers `true` outright —
     nothing automatic is ever coming either way — and so does a goal that
     has never once had an orchestrator session: nothing automatic has
     touched it yet. Otherwise, with at least one orchestrator session on
     record, the goal's own give-up mark is read:
     `Goal::orchestrator_given_up_at`, stamped by
     `scheduler::goals::orchestrator_could_not_start` once the spawn-retry
     budget actually runs out and cleared the moment `keep_orchestrator`
     has a live orchestrator again or a resume/spawn succeeds, answers
     `true` on its own. This is deliberately not the `disconnected` flag
     the liveness sweep's `retire_disconnected` also raises on every
     ordinary crash, well before the budget is spent and while a relaunch
     may still be coming — reading that flag as a give-up was exactly the
     false positive an earlier round of this list raised. One task failed
     on the daemon's own descriptor-limit words is `resource`, grouped
     into one item naming every such task — one machine out of
     descriptors, not one item per task it happened to fail. Every other
     one is its own `unknown` item, naming that task alone and carrying
     its own ended reason as the summary, rather than guessed into a group
     with no reliable evidence it shares a cause with another.
   - every goal whose orchestrator has been given up on
     (`Goal::orchestrator_given_up_at`) and which has no failed task of
     its own: a taskless orchestrator — one still planning, with nothing
     yet to fail — has no route onto the bullet above, since that one
     only ever walks failed tasks, so `orchestrator_given_up_items` gives
     it one of its own, naming the goal and targeting the alarm session
     the give-up itself is raised on (or the goal's last orchestrator
     session where none still carries it). A goal that also has a failed
     task is left entirely to the bullet above, so the one give-up is not
     said twice.
   - every pull request whose reviewer session has been given up on
     (`PullRequestRow::reviewer_given_up_at`, the same kind of mark,
     stamped by `scheduler::pull_requests::start_pull_request_session`
     once *its* spawn-retry budget runs out and cleared the moment a
     resume or spawn of that session next succeeds): a reviewer session
     sits on no task either, and carries the same `disconnected` false-
     positive risk the orchestrator's own mark exists to avoid.
     `reviewer_given_up_items` gives it its own item, targeting the
     request itself.
   - `configuration`: every enabled forge integration whose `fetch_error`
     names the fixed words [`crate::forge::Cli::binary`] gives a CLI
     missing from the daemon's PATH, grouped by the CLI name the message
     itself names — the fix is the same install wherever it is missing
     from. Any other fetch error — besides the `access` one below —
     raises nothing: `forge/poll.rs` retries it forever with no budget to
     spend, so without a confirming check there is no evidence here that
     recovery has given up rather than still trying, and raising every
     retryable forge hiccup as a human blocker would be exactly the
     speculative classification this producer does not make. A disabled
     integration's fetch error is nobody's business: turning the
     integration off is itself the fix.
   - `access`: an enabled forge integration whose `fetch_error` carries
     `crate::forge::poll::FORGE_SIGNED_OUT`, grouped by host — the fix,
     signing back in, is the same wherever that host's repositories are.
     `forge/poll.rs` writes the marker reactively, only once a fetch has
     already failed, and only on a *confirmed* rejection:
     `ForgeClient::confirmed_signed_out` runs the forge CLI's own
     `auth status` and reads `Refusal::ran()` — whether the CLI actually
     reached an exit code, however it answered — rather than merely
     whether the check returned an error. A missing binary, a spawn or
     write failure, or a timeout never ran at all, so it answers `false`
     and the marker is never written: `configuration`'s own cause, or
     nothing, is left to say why the fetch failed instead, and `access`
     is reserved for the CLI having actually run and said no. Checking an
     existing status after a failure that already happened is not a new
     watch over the forge — it is one answer, read once, to the one
     question "is the fetch failing because the CLI is signed out" — so
     it stays inside the "no new monitoring service" line rule 4's
     `configuration` cause already holds for a fetch error with no such
     confirmation.
5. `access` is produced by the recovery path above the moment the daemon's
   own reactive sign-in check confirms it; naming one from a forge CLI's
   free-text error alone, or from the advisory failure classifier (024),
   would still be exactly the speculative classification this list does
   not raise, and recovery does neither.
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
   `waiting_user`, `agent_error`), with the following suppressed so the
   two lists never say the same thing twice, or raise a row while
   automatic recovery is still trying:
   - a session's bare `exhausted` flag never raises a row of its own on
     either client; only a `quota` item does, read with every other
     reason in the same pass rather than used to gate a locally-derived
     row — a session automatic switching is still working on is not on
     the list at all, under any reason;
   - a task's own bare `failed` status never raises a row either, once
     the recovery read is `complete`: whatever this producer has to say
     about a failed task — its own named, actionable item, or silence
     while its orchestrator is still the one answering it — is
     authoritative, and the local `failed` reason defers to it entirely
     (`group`'s `recovery_trustworthy` in the CLI,
     `recoveryTrustworthy` in Desktop). A membership check on which
     tasks a recovery item happens to name would not do: an empty,
     `complete` recovery read is what suppression while the orchestrator
     still owns the failure looks like, and reads identically to "this
     task was never considered" unless `complete` itself is read instead.
     The local `failed` reason survives only as the fallback a recovery
     read that could not be read in full (`complete: false`) leaves
     standing, so a task's failure is never silently dropped just because
     the producer that would otherwise explain it had a hiccup;
   - a task's `stalled` flag never raises a row, on the task or on its
     session: it is raised while automatic recovery (a nudge, then a
     relaunch) is still working the agent, and reporting it immediately
     would be exactly the "automatic recovery still trying" row this
     contract asks never to raise. If the relaunch also fails, the task
     fails, and the point above covers it;
   - a session's `disconnected` flag never raises a row of its own
     either, task-tied or not: the scheduler resumes a disconnected agent
     automatically, whatever it runs, and the flag by itself is exactly
     the same one a mere crash raises well before any retry budget is
     spent (`retire_disconnected`) — raising it locally would be the
     false positive recovery's own `given_up_at` marks exist to avoid.
     A task-tied session's eventual failure is the point above's
     business; a taskless orchestrator's or reviewer's eventual give-up
     is the recovery producer's own `unknown` item
     (`orchestrator_given_up_items`, `reviewer_given_up_items`), read with
     every other recovery item below — never derived from the bare flag
     here, on either client, for either seat.

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
  (`attention.rs::a_quota_items_affected_entry_names_the_sessions_task`), and
  a reviewer session's names its seat the same as an orchestrator's
  (`attention.rs::a_quota_items_affected_entry_names_a_reviewer_session_by_seat`);
  two sessions on the same model blocked for two different reasons stay two
  separate items, each with the right session and the right action
  (`attention.rs::two_sessions_on_the_same_model_blocked_for_different_reasons_stay_separate_items`);
  one already switched to a successor raises nothing
  (`attention.rs::an_exhausted_session_already_switched_raises_no_quota_item`);
  and one nobody is waiting on raises nothing even while exhausted
  (`attention.rs::an_exhausted_session_nobody_is_waiting_on_raises_no_quota_item`).
- A failed task raises nothing while its orchestrator has not yet had a
  real turn on it, proven end to end through the real scheduler and a
  stub agent rather than a hand-written stamp: queuing the prompt is not
  enough on its own
  (`attention.rs::a_failed_tasks_item_waits_for_its_orchestrator_to_actually_have_a_turn_on_it`).
  Confirmation is read by this task's own id, never by whether the goal
  has any confirmed turn at all — a turn that confirmed a different
  failure does not answer for this one
  (`attention.rs::a_failed_tasks_item_still_waits_while_only_a_different_failure_was_confirmed`).
  A goal whose dead orchestrator has not yet been given up on still
  raises nothing, since `keep_orchestrator` may still relaunch it
  (`attention.rs::a_failed_task_raises_nothing_while_its_dead_orchestrator_has_not_been_given_up_on`);
  a mere crash is not read as that give-up either — the `disconnected`
  flag a crash alone raises answers for nothing on its own
  (`attention.rs::a_failed_task_raises_nothing_while_its_orchestrator_merely_crashed_without_giving_up`);
  and the item raises once the goal's own give-up mark is actually set
  (`attention.rs::a_failed_task_raises_its_item_once_its_orchestrator_is_given_up_on`).
  Retrying a failed task takes its item down without losing the
  transition that recorded why it failed
  (`attention.rs::retrying_a_failed_task_removes_its_item_without_losing_the_transition`).
- A taskless orchestrator given up on — a goal still planning, with no
  failed task of its own — raises its own `unknown` item end to end
  through the real scheduler's spawn-retry budget actually running out,
  and that item answers for it even though the same crash has already
  raised the generic `disconnected` flag on the alarm row
  (`scheduler_attention.rs::an_orchestrator_that_dies_the_moment_it_starts_is_given_up_on`).
- A forge integration whose fetch failed on the daemon's own "CLI not
  installed" words is a `configuration` item naming the repository
  (`attention.rs::a_missing_forge_cli_is_a_configuration_item`); one whose
  fetch failed on anything else raises nothing, since the daemon keeps
  retrying it with no evidence recovery has given up
  (`attention.rs::a_forge_fetch_error_that_names_no_missing_cli_raises_nothing`);
  a disabled integration's fetch error raises nothing either
  (`attention.rs::a_disabled_forge_integrations_fetch_error_raises_nothing`).
  One whose fetch failed and whose stored error carries the daemon's own
  reactive sign-in confirmation is an `access` item naming the host
  (`attention.rs::a_signed_out_forge_cli_is_an_access_item`), produced
  end to end through the real forge poll worker and a scripted CLI that
  fails both `pr list` and `auth status`
  (`attention.rs::a_forge_cli_signed_out_mid_fetch_is_confirmed_reactively_and_reaches_the_list`).
  The marker is read from whether the CLI actually ran that check, not
  merely whether it returned an error, so a missing binary or a timeout
  on the check itself never gets misread as a confirmed rejection
  (`forge/mod.rs::a_refusal_has_run_only_where_the_cli_reached_an_exit_code`,
  and `::a_cli_that_reads_no_input_is_stopped_by_the_deadline` for a real
  timeout's own `Refusal::ran()` reading `false`).
- The same recovery item answers the same id across two reads, and that id
  is derived from the shared cause alone
  (`attention.rs::a_recovery_items_id_is_stable_across_two_reads`), including
  across a second, independent store connection opened on the same database
  file — the closest this suite comes to a daemon restart, since nothing
  else in it spawns a second daemon process either
  (`attention.rs::a_recovery_items_id_is_stable_across_a_fresh_store_connection`).
- The CLI board raises no `exhausted` row of its own, and no `disconnected`
  or `stalled` row either, task-tied or not
  (`board.rs::an_exhausted_session_raises_no_row_of_its_own_on_this_board`,
  `::a_taskless_disconnected_or_stalled_session_raises_no_row_of_its_own_on_this_board`,
  `::a_flagged_session_row_spells_the_reason_the_ui_spells`,
  `::a_row_names_what_it_is_about`); every recovery item, `quota` included,
  prints in its own section with its reason, its waiting time, its action
  and what it affects
  (`attention.rs::every_recovery_item_prints_in_its_own_section`); a
  failed task a complete recovery read has nothing to say about stays off
  the board entirely, rather than falling back to its bare `failed` row,
  and only an incomplete read brings that fallback back
  (`board.rs::a_failed_task_recovery_has_not_named_stays_off_the_board_once_recovery_is_trustworthy`);
  every affected entry's own id leads its line, so each stays reachable even
  inside a grouped item; `-q` names every recovery item's own id alongside
  the board's
  (`attention.rs::quiet_rows_also_names_every_recovery_item`); and `-q`
  fails the command on an incomplete read rather than succeeding silently
  (`attention.rs::quiet_mode_fails_on_an_incomplete_recovery_read`).
- Ariadne Desktop's attention list raises no row for an exhausted session
  recovery has not given up on, raises one once a `quota` item names it,
  counts a resource or configuration item that names no goal or session,
  does not also count a task's own row once a recovery item already names
  it, raises no row for a failed task once a complete recovery read
  answers with nothing to say about it — rather than falling back to the
  bare status the moment a membership check on the read's own items came
  up empty —, and raises no row of its own for a stalled or disconnected
  session, task-tied or not — a taskless orchestrator's `disconnected`
  flag included, which only raises a row once recovery's own give-up
  item names its goal, never from the bare flag
  (`ui/src/features/goals/attention.test.tsx`, "raises no row for a
  failed task once a complete recovery read has nothing to say about
  it", "raises no row of its own for a taskless orchestrator's
  disconnected flag", "raises a row for a taskless orchestrator once
  recovery's own give-up item names its goal"). A grouped quota item with
  two or more affected sessions gives each its own link to its own
  session's terminal — asserted against the
  exact destination `sessionTerminalFrom` itself builds, not only the id
  copied into the helper's result — and one with a single session adds no
  second line saying the same thing the row's own link already does
  (`attention.test.tsx::gives_a_grouped_quota_items_every_session_its_own_link_to_its_own_terminal`,
  `::gives_a_single-session_quota_item_no_extra_links_of_its_own`).
- Neither client reads an incomplete or failed read as all-clear: the CLI
  notes it instead of printing the empty state and fails under `-q`, and
  Desktop's title and badge read unavailable rather than quiet while zero
  items sit under an unfinished read
  (`ui/src/features/goals/attention.test.tsx`, "never reads an incomplete
  recovery read as nothing needing attention";
  `attention.rs::quiet_mode_fails_on_an_incomplete_recovery_read`).

## Known gap

- `agent_requests` and `pull_requests` are registered producer modules
  that answer empty: the question and pull-request eligibility rules this
  route was built for, and migrating the client behavior that already
  covers them onto it, are a later task's.

## Sources

`crates/ariadne-api/src/attention.rs`, `crates/ariadne-daemon/src/attention/`
(`mod`, `recovery` — including `orchestrator_has_answered_for`,
`access_items`, `orchestrator_given_up_items`, `reviewer_given_up_items` —
`agent_requests`, `pull_requests`), `crates/ariadne-daemon/src/http/attention.rs`,
`crates/ariadne-daemon/src/http/events.rs` (`ingest_event`'s
`promote_goal_orchestrator_told` call on an orchestrator's own `stop`),
`crates/ariadne-daemon/src/scheduler/auto_switch.rs` (`recovery_block`,
`switch_chain`, `switch_target`), `crates/ariadne-daemon/src/scheduler/goals.rs`
(`tell_orchestrator`, `keep_orchestrator`, `orchestrator_could_not_start`),
`crates/ariadne-daemon/src/scheduler/pull_requests.rs`
(`start_pull_request_session`, `review_pass`),
`crates/ariadne-store/src/goals.rs` (`set_goal_orchestrator_told`,
`promote_goal_orchestrator_told`, `set_goal_orchestrator_given_up`,
`clear_goal_orchestrator_given_up`, `Goal::orchestrator_told_failed_task_ids`,
`Goal::orchestrator_answered_failed_task_ids`, `Goal::orchestrator_given_up_at`),
`crates/ariadne-store/src/pull_requests.rs` (`set_pull_request_reviewer_given_up`,
`clear_pull_request_reviewer_given_up`, `PullRequestRow::reviewer_given_up_at`),
`crates/ariadne-daemon/src/forge/mod.rs` (`ForgeClient::confirmed_signed_out`,
`Refusal::ran`), `crates/ariadne-daemon/src/forge/poll.rs` (`FORGE_SIGNED_OUT`),
`crates/ariadne-cli/src/commands/attention.rs`,
`crates/ariadne-cli/src/commands/attention/board.rs`,
`ui/src/features/goals/attention.ts`, `ui/src/features/goals/attention-alerts.tsx`,
`ui/src/features/goals/attention-strip.tsx`.
