---
id: needs-attention
status: current
updated: 2026-10-10
areas: [api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/attention.rs
  - crates/ariadne-daemon/src/attention/recovery.rs
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
recovery — which covers a model's quota, a machine resource shortage, a
daemon configuration blocker, and a forge failure neither names.

Out: the recovery semantics themselves — the auto-switch ladder, the
spawn-retry budget, the descriptor-limit detection — which are 009's. An
agent's own request and a pull request's next step, which stay on the
question and pull-request surfaces (009, 026, 029) until a later task gives
their own producers (registered already, empty — rule 9) their eligibility
rules.

## Behavior

1. `GET /v1/attention` answers `AttentionListDto { items, complete }`.
   `items` is every item every registered producer currently finds;
   `complete` is false where a producer's read failed, so a partial read
   never answers as if nothing were wrong (a failed producer costs the
   list its own items, not another producer's).
2. An item (`AttentionItemDto`) carries a stable `id`, which producer
   raised it (`producer`: `recovery`, and not yet `agent_request` or
   `pull_request` — rule 9), a `cause` (`access`, `quota`, `configuration`,
   `resource`, or `unknown`), a one-line `summary`, the one
   `required_action` that clears it, `since` (RFC 3339, first observed),
   every `affected` subject (`goal`, `task`, `session` or `repository`,
   with an id and a label), and a typed `target` (`console`, `task`,
   `pull_request` or `settings`) a client opens the item onto. Opening a
   target does not resolve the item — only the action it names does.
   `producer` and `cause` answer different questions: a later producer may
   share none of recovery's causes, or raise `unknown` for a reason a
   client still has to tell apart from recovery's own.
3. Items group only on reliable, already-computed evidence of a shared
   cause, never on a guess at one: every session exhausted on the same
   model, every task failed on the exact descriptor-limit words
   (`scheduler::DESCRIPTOR_LIMIT_REASON`), every repository whose forge
   fetch failed naming the same missing CLI. An `unknown` item is never
   grouped with another, unknown or not — there being no reliable evidence
   it shares a cause with anything is exactly why it reads `unknown`. An
   item's id is derived from its shared key, not issued fresh, so the same
   blocker is the same row across a restart and across the same cause
   recurring on a later poll.
4. The recovery producer (`crate::attention::recovery`) raises its items
   from evidence the daemon already keeps for its own retry loops, never
   from a guess:
   - `quota`: a session flagged `exhausted` (009), exited, whose work is
     still active (`crate::attention::work_is_active`), grouped with
     every other one exhausted on the same model. Raised only once
     automatic recovery has spent its options on it —
     `scheduler::auto_switch::recovery_exhausted` answers `false` (no
     item) while a successor session is already running the work, while
     automatic switching is enabled and has not yet spent the
     spawn-retry budget on it, or while a candidate model remains to try;
     it answers `true` (an item) only once none of those still holds.
     This is the same question `Scheduler::auto_switch_exhausted` asks
     before acting, read as a fact rather than acted on, so the item and
     the switch it waits on can never disagree about which models are
     still candidates — propagating a store error rather than collapsing
     it to "no candidate", since a producer that cannot tell a genuine
     exhaustion from a store that would not answer must say so in
     `complete` rather than raise a false item.
   - `resource`: every `failed` task whose ended reason
     (`Store::ended_reason`) is exactly `DESCRIPTOR_LIMIT_REASON`, grouped
     into one item naming every such task — one machine out of
     descriptors, not one item per task it happened to fail. A task this
     item already names is left off the client's own composed board
     entirely (rule 7): the generic `failed` row would say the same
     failure a second time, with neither the cause nor the action the
     recovery item carries.
   - `configuration` and `unknown`: every enabled forge integration whose
     `fetch_error` is set. One naming the fixed words
     [`crate::forge::Cli::binary`] gives a CLI missing from the daemon's
     PATH is `configuration`, grouped by the CLI name the message itself
     names — the fix is the same install wherever it is missing from. Any
     other fetch error is kept rather than dropped for want of a pattern
     to name it by: its own `unknown` item, naming that one repository,
     never grouped with another (rule 3). A disabled integration's fetch
     error is nobody's business: turning the integration off is itself
     the fix.
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
   route does not yet cover (`waiting_permission`, `waiting_input`,
   `waiting_user`, `agent_error`, `disconnected`, `stalled`), with two
   adjustments so the two lists never say the same thing twice:
   - a session's bare `exhausted` flag never raises a row of its own on
     either client; only a `quota` item does, read with every other cause
     in the same pass rather than used to gate a locally-derived row — a
     session automatic switching is still working on is not on the list
     at all, under any reason;
   - a task any recovery item names in its `affected` list (today, only
     `resource`) is left off the client's own composed rows, whatever
     the local reason would have been, since the recovery item is both
     more specific and already on the list.

   Every recovery item is its own row, with no goal of its own the way a
   composed task or session row has one: the CLI's `RECOVERY` section
   below the per-goal board, and a Desktop strip row synthesized straight
   from the item (`AttentionItem.recovery`), rendering the item's own
   summary, action, since, cause and `affected` list rather than a label
   derived from a bare flag.
8. A client never reads a partial or failed read as "nothing needs
   attention": a query failure, or `complete: false` with no items, holds
   the list open rather than letting it read as all-clear. The CLI prints
   a note instead of the empty state and marks `complete` in its own
   `--format json`; Desktop folds `complete: false` into the same
   `error`/`partial` path a query failure already takes (one code path,
   not two to keep in step), and the shell's window title and sidebar
   badge — not only the strip — read `(!)`/`!` rather than the quiet
   state while zero items sit under an unfinished read.
9. Registering a further producer is adding its own module beside
   `recovery` (`crate::attention::<name>`) and a call to it from
   `crate::attention::collect`; nothing about the route or the DTO
   changes to add one. `agent_requests` and `pull_requests` are
   registered already, each answering an empty list, so the route a later
   task's producer and client migration need already compiles and runs;
   the question and pull-request eligibility rules themselves are out of
   this spec's scope (see Scope).

## Acceptance criteria

- The route answers an empty, complete list when nothing is stuck, and is
  in the API document under the `attention` tag
  (`attention.rs::an_empty_daemon_answers_an_empty_complete_list`).
- A task failed on the descriptor-limit words is a `resource` item naming
  that task (`attention.rs::a_descriptor_limit_failure_is_a_resource_item`),
  two tasks sharing the same words are one grouped item
  (`attention.rs::two_tasks_sharing_a_descriptor_limit_failure_are_one_grouped_item`),
  and a task failed for any other reason raises nothing
  (`attention.rs::a_task_failed_for_another_reason_raises_no_resource_item`).
- An exhausted session on a model the catalog does not rank — so automatic
  switching has nothing to try — is a `quota` item naming that session,
  targeting its console
  (`attention.rs::an_exhausted_session_on_an_unranked_model_is_a_quota_item`),
  naming the producer as `recovery`; one already switched to a successor
  raises nothing
  (`attention.rs::an_exhausted_session_already_switched_raises_no_quota_item`);
  and one nobody is waiting on raises nothing even while exhausted
  (`attention.rs::an_exhausted_session_nobody_is_waiting_on_raises_no_quota_item`).
- A forge integration whose fetch failed on the daemon's own "CLI not
  installed" words is a `configuration` item naming the repository
  (`attention.rs::a_missing_forge_cli_is_a_configuration_item`); one whose
  fetch failed on anything else is its own `unknown` item rather than
  dropped
  (`attention.rs::an_unmatched_forge_fetch_error_is_its_own_unknown_item`),
  and two such repositories stay two separate `unknown` items rather than
  one grouped guess
  (`attention.rs::two_unmatched_forge_errors_stay_separate_unknown_items`);
  a disabled integration's fetch error raises nothing
  (`attention.rs::a_disabled_forge_integrations_fetch_error_raises_nothing`).
- The same recovery item answers the same id across two reads
  (`attention.rs::a_recovery_items_id_is_stable_across_two_reads`).
- The CLI board raises no `exhausted` row of its own on any flag — not
  even one `GET /v1/attention` confirms, since that reads from the
  recovery section instead
  (`board.rs::an_exhausted_session_raises_no_row_of_its_own_on_this_board`,
  `::a_flagged_session_row_spells_the_reason_the_ui_spells`); every
  recovery item, `quota` included, prints in its own section with its
  cause, its waiting time, its action and what it affects
  (`attention.rs::every_recovery_item_prints_in_its_own_section`); and a
  task any recovery item names is left off the board's own rows, whatever
  its cause
  (`attention.rs::recovery_affected_task_ids_names_every_cause`).
- Ariadne Desktop's attention list raises no row for an exhausted session
  recovery has not given up on, raises one once a `quota` item names it,
  counts a resource or configuration item that names no goal or session,
  and does not also count a task's own row once a recovery item already
  names it
  (`ui/src/features/goals/attention.test.tsx`).
- Neither client reads an incomplete or failed read as all-clear: the CLI
  notes it instead of printing the empty state, and Desktop's title and
  badge read unavailable rather than quiet while zero items sit under an
  unfinished read (`ui/src/features/goals/attention.test.tsx`, "never reads
  an incomplete recovery read as nothing needing attention").

## Known gap

- No `access`/credential cause is produced yet: no evidence in the daemon
  today tells a transient forge auth hiccup from one recovery has
  genuinely given up on. A later producer with such evidence may use the
  `access` cause already in the vocabulary.
- `agent_requests` and `pull_requests` are registered producer modules
  that answer empty: the question and pull-request eligibility rules this
  route was built for are a later task's, and the CLI and Desktop UI's
  own composed lists still carry that behavior unchanged until it lands.
- A session stalled, disconnected, or carrying an agent error is still
  read entirely off the client's own composed list (rule 7), not this
  route: `stalled`'s escalation is the watchdog thresholds of 009, already
  tuned for when a person should be told, and `disconnected`/`agent_error`
  are already raised only once the daemon's own resume or nudge has
  nothing left to try (009 rules 19, 22-23) — so showing them immediately
  is not the "automatic recovery still trying" row this list exists not to
  raise. Moving them onto this route, if it is ever worth doing, is a
  question for 009's own thresholds, which this task does not touch.

## Sources

`crates/ariadne-api/src/attention.rs`, `crates/ariadne-daemon/src/attention/`
(`mod`, `recovery`, `agent_requests`, `pull_requests`),
`crates/ariadne-daemon/src/http/attention.rs`,
`crates/ariadne-daemon/src/scheduler/auto_switch.rs` (`recovery_exhausted`,
`switch_chain`, `switch_target`), `crates/ariadne-cli/src/commands/attention.rs`,
`ui/src/features/goals/attention.ts`, `ui/src/features/goals/attention-alerts.tsx`.
