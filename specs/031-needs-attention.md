---
id: needs-attention
status: current
updated: 2026-10-10
areas: [api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/attention.rs
  - crates/ariadne-daemon/src/attention/recovery.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/src/commands/attention/board.rs
  - ui/src/features/goals/attention.test.tsx
---

# Needs attention

One list of what a person has something to do about right now, read off the
daemon rather than inferred by a client from a task's status or a session's
bare flag.

## Scope

In: the shared item shape, `GET /v1/attention`, the producer registry
(`crate::attention` in `ariadne-daemon`), and the first producer — recovery
— which covers a model's quota, a machine resource and a daemon
configuration blocker.

Out: the recovery semantics themselves — the auto-switch ladder, the
spawn-retry budget, the descriptor-limit detection — which are 009's. An
agent's own request and a pull request's next step, which stay on the
question and pull-request surfaces (009, 026, 029) until a later task moves
them onto this list.

## Behavior

1. `GET /v1/attention` answers `AttentionListDto { items, complete }`.
   `items` is every item every registered producer currently finds;
   `complete` is false where a producer's read failed, so a partial read
   never answers as if nothing were wrong (a failed producer costs the
   list its own items, not another producer's).
2. An item (`AttentionItemDto`) carries a stable `id`, a `cause`
   (`access`, `quota`, `configuration`, `resource`, or `unknown`), a
   one-line `summary`, the one `required_action` that clears it, `since`
   (RFC 3339, first observed), every `affected` subject (`goal`, `task`,
   `session` or `repository`, with an id and a label), and a typed
   `target` (`console`, `task`, `pull_request` or `settings`) a client
   opens the item onto. Opening a target does not resolve the item —
   only the action it names does.
3. Items group only on reliable, already-computed evidence of a shared
   cause, never on a guess at one: every session exhausted on the same
   model, every task failed on the exact descriptor-limit words
   (`scheduler::DESCRIPTOR_LIMIT_REASON`), every repository whose forge
   fetch failed naming the same missing CLI. An item's id is derived from
   that shared key, not issued fresh, so the same blocker is the same row
   across a restart and across the same cause recurring on a later poll.
4. The recovery producer (`crate::attention::recovery`) raises three kinds
   of item from evidence the daemon already keeps for its own retry
   loops:
   - `quota`: a session flagged `exhausted` (009), exited, whose work is
     still active (`crate::attention::work_is_active`), once its own
     switch chain has no further candidate and no switch is already under
     way (`scheduler::auto_switch::recovery_exhausted`) — the same three
     conditions the auto-switch sweep itself gives up on, read rather than
     acted on. A session mid-switch raises nothing: automatic recovery is
     still trying it.
   - `resource`: every `failed` task whose ended reason
     (`Store::ended_reason`) is exactly `DESCRIPTOR_LIMIT_REASON`, grouped
     into one item naming every such task — one machine out of
     descriptors, not one item per task it happened to fail.
   - `configuration`: every enabled forge integration whose
     `fetch_error` contains the fixed words
     [`crate::forge::Cli::binary`] gives a CLI missing from the daemon's
     PATH, grouped by the CLI name the message itself names. A disabled
     integration's fetch error is nobody's business.
5. An access or credential cause (`access`) has no deterministic evidence
   yet and is not produced by this path; naming one from a forge CLI's own
   free-text error, or from the advisory failure classifier (024), would be
   exactly the speculative classification this list does not raise.
   `unknown` is likewise unused today — both are part of the shared
   vocabulary so a later producer can use them without a schema change,
   and an item of either kind is never grouped with another, unknown or
   not.
6. A client reads this list live by invalidating it on the same SSE events
   that already carry its evidence — `session_updated`, `task_updated` and
   `repository_updated` — rather than a dedicated event: the recovery
   producer's reads are cheap enough that a fresh `GET /v1/attention` on
   each of those is simpler than patching a derived list by hand.
7. The CLI (`ariadne attention`) and Ariadne Desktop (the attention strip,
   its badge and its toasts) each still compose their own list from goals,
   tasks and sessions (009's existing contract), with one exception: a
   session's bare `exhausted` flag no longer raises a row on its own.
   `exhausted` reads as a row only once a `quota` item names the session,
   so a session automatic switching is still working on raises nothing
   while it is still being tried. Every other recovery cause — `resource`,
   `configuration` — names no goal or session a composed row already
   covers, and is added as its own row (CLI: a `RECOVERY` section below the
   per-goal board; Desktop: a strip row naming no goal, via a synthesized
   `AttentionItem`).
8. Registering a further producer is adding its own module beside
   `recovery` (`crate::attention::<name>`) and a call to it from
   `crate::attention::collect`; nothing about the route, the DTO or a
   client's consumption of it changes to add one. The question and
   pull-request eligibility rules a later task gives their own producers
   are out of this spec's scope (see Scope).

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
  and one nobody is waiting on raises nothing even while exhausted
  (`attention.rs::an_exhausted_session_nobody_is_waiting_on_raises_no_quota_item`).
- A forge integration whose fetch failed on the daemon's own "CLI not
  installed" words is a `configuration` item naming the repository
  (`attention.rs::a_missing_forge_cli_is_a_configuration_item`), and a
  disabled integration's fetch error raises nothing
  (`attention.rs::a_disabled_forge_integrations_fetch_error_raises_nothing`).
- The same recovery item answers the same id across two reads
  (`attention.rs::a_recovery_items_id_is_stable_across_two_reads`).
- The CLI board raises no `exhausted` row until `GET /v1/attention` names
  the session, and changes nothing for a quota item naming a different one
  (`attention.rs::an_exhausted_session_is_a_row_only_once_recovery_has_named_it`,
  `::only_quota_items_name_a_recovery_exhausted_session`), and a resource
  or configuration item prints as its own section
  (`::recovery_items_other_than_quota_print_as_their_own_section`).
- Ariadne Desktop's attention list raises no row for an exhausted session
  automatic switching is still trying, raises one once a `quota` item names
  it, and folds a resource or configuration item into its own row naming
  no goal
  (`ui/src/features/goals/attention.test.tsx`).

## Known gap

- No `access`/credential cause is produced yet: no evidence in the daemon
  today tells a transient forge auth hiccup from one recovery has
  genuinely given up on. A later producer with such evidence may use the
  `access` cause already in the vocabulary.
- The question and pull-request producers this route was built for (the
  hand-off this spec exists to support) are not implemented here; the CLI
  and Desktop UI's own composed lists still carry that behavior unchanged.

## Sources

`crates/ariadne-api/src/attention.rs`, `crates/ariadne-daemon/src/attention/`
(`mod`, `recovery`), `crates/ariadne-daemon/src/http/attention.rs`,
`crates/ariadne-daemon/src/scheduler/auto_switch.rs` (`recovery_exhausted`),
`crates/ariadne-cli/src/commands/attention.rs`, `ui/src/features/goals/attention.ts`.
