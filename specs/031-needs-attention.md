---
id: needs-attention
status: current
updated: 2026-10-11
areas: [api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/attention.rs
  - crates/ariadne-daemon/tests/it/scheduler_attention.rs
  - crates/ariadne-daemon/tests/it/pull_request_reviews.rs
  - crates/ariadne-daemon/tests/it/kept_requests.rs
  - crates/ariadne-daemon/tests/it/workflow_pull_request.rs
  - crates/ariadne-daemon/tests/it/pull_requests.rs
  - crates/ariadne-daemon/src/attention/recovery.rs
  - crates/ariadne-daemon/src/attention/pull_requests.rs
  - crates/ariadne-daemon/src/attention/mod.rs
  - crates/ariadne-daemon/src/acp.rs
  - crates/ariadne-daemon/src/scheduler/auto_switch.rs
  - crates/ariadne-daemon/src/scheduler/goals.rs
  - crates/ariadne-daemon/src/scheduler/pull_requests.rs
  - crates/ariadne-daemon/src/scheduler/quiet.rs
  - crates/ariadne-daemon/src/scheduler/tasks.rs
  - crates/ariadne-daemon/src/forge/mod.rs
  - crates/ariadne-daemon/src/forge/poll.rs
  - crates/ariadne-daemon/src/forge/github/pulls.rs
  - crates/ariadne-daemon/src/forge/gitlab/pulls.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/src/commands/attention/board.rs
  - ui/src/features/goals/attention.test.tsx
  - ui/src/features/goals/attention-alerts.test.tsx
  - ui/src/events/dispatch.test.ts
  - ui/src/features/pull-requests/pull-request-panel.test.tsx
---

# Needs attention

One list of what a person has something to do about right now, read off the
daemon rather than inferred by a client from a task's status or a session's
bare flag.

## Scope

In: the shared item shape, `GET /v1/attention`, the producer registry
(`crate::attention` in `ariadne-daemon`), the first complete producer —
recovery — which covers a model's quota, every task the daemon will never
retry on its own, and a forge CLI missing from the daemon's PATH, and the
second — `pull_requests` — which covers a review request nobody is
assigned to and a request a babysitting task has confirmed ready to merge
(rules 11-14), migrating the "ready to merge" / "review posted" session
surface onto it and removing it from 029's own `waiting_user` rule.

Out: the recovery semantics themselves — the auto-switch ladder, the
spawn-retry budget, the watchdog thresholds — which are 009's; the review
and readiness semantics themselves — which row wants a session, the
verdict policy, `report_pull_request`'s own fields — which are 029's. An
agent's own request, which stays on the question surface (009) until a
later task gives its own producer (registered already, empty — rule 10)
its eligibility rules.

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
     already happened from evidence tied to exactly *this* occurrence of
     this task's failure — its id *and* the id of the `task_transitions`
     row that most recently moved it to `failed`
     (`Store::latest_transition_to`) — never from `Task::updated_at`,
     which is mutable metadata: `Store::update_task` moves it when a
     failed task's title, description or staffing is merely edited, a
     change that has nothing to do with the failure itself, and a
     confirmation keyed on it would vanish — taking its recovery item down
     with it — the moment anyone edited the task. `tell_orchestrator` tags
     the queued prompt with the `(id, transition id)` pair of every task
     it names (`acp::Delivery::GoalAttention`), and the ACP driver writes
     that exact pair into `Goal::orchestrator_answered_failed_task_ids`
     only once that one prompt's own `session/prompt` call returns — the
     turn that actually carried it, confirmed from the delivery that
     completed, not inferred from any session reaching `stop`
     (`acp::serve_with_input`). An unrelated turn landing on the same
     session first, whatever it carried, writes nothing here, since only
     a completed `GoalAttention` delivery ever does; and a task retried
     off `failed` and failed again stamps a fresh transition row its old
     confirmation does not carry, so that confirmation does not answer for
     the new occurrence. The gate answers `true` once this exact pair is in
     the confirmed list.

     The scheduler's own in-memory `goal_told` cache — keyed on the
     rendered situation text alone, which names only a task's title, id
     and `failed` status — is never trusted by itself to skip a resend:
     two failures of the same task in a row render the identical text, and
     a delivery whose write failed or whose turn never finished before its
     connection ended (the "unwritten"/release branch in
     `acp::serve_with_input`, which has nothing of its own to release for
     a `GoalAttention` delivery — nothing was claimed ahead of the write)
     leaves the cache believing a failure was told that never actually
     confirmed. `tell_orchestrator` only skips a resend where the cache
     *and* the persisted confirmed list agree — every currently failed
     task's `(id, transition id)` pair is already in
     `Goal::orchestrator_answered_failed_task_ids` — so an unconfirmed or
     abandoned delivery is retried on every later pass, through the
     session's next automatic resume, until a turn actually completes and
     confirms it. A goal not `orchestrated`, or not `planning` or
     `active` any more, answers `true` outright — nothing automatic is
     ever coming either way. Otherwise the goal's own give-up mark is
     read: `Goal::orchestrator_given_up_at`, stamped by
     `scheduler::goals::orchestrator_could_not_start` once the spawn-retry
     budget actually runs out, *or* by the watchdog's own exhausted-
     relaunch decision for a taskless session (`scheduler::quiet::relaunch_wedged`,
     which has no task of its own to fail onto instead, and deliberately
     leaves the wedged process alive rather than killing it) — answers
     `true` on its own, whether or not an orchestrator session exists this
     instant: an orchestrated goal still waiting on its first launch is not
     one with nothing coming either. The mark is cleared only on genuine
     recovery, never merely because a session is live: `keep_orchestrator`
     clears it where a resume or spawn succeeds, or where a live session's
     own `last_activity_at` has moved past the moment the mark was set —
     evidence the session actually reported something since, which a
     session `relaunch_wedged` left standing exhausted, by construction,
     cannot produce on its own. Clearing it merely because the session is
     found "live" would flap the mark — and the item with it — every pass
     that finds the same still-wedged, still-silent session: `relaunch_wedged`
     sets it again with a fresh `since`, both writes publish invalidations,
     and a reader caught between them sees no blocker despite recovery
     having exhausted its options, while a later reader sees a younger
     blocker that may have been removed and re-announced as the same item.
     `give_up_summary` also tells the two ways a taskless session gives up
     apart in its wording — "stopped answering after N relaunches" for
     `relaunch_wedged`'s own exhausted-relaunch decision, "would not start
     after N attempts" for `orchestrator_could_not_start`'s/
     `start_pull_request_session`'s own spawn-retry exhaustion — read from
     a `wedged` flag persisted *beside* the give-up mark itself
     (`Goal::orchestrator_given_up_wedged`,
     `PullRequestRow::reviewer_given_up_wedged`, written once with the
     mark and kept with its `since`), never inferred from the alarm
     session's own `stalled` flag: `scheduler::quiet::check_session_quiet`
     checks its relaunch threshold before its flag one, so a pass that
     first observes a session already past the relaunch threshold gives
     up without ever having raised the flag at all, and a `stalled`-flag
     read would then wrongly call a session that started and went quiet
     one that "would not start". The two counts differ too:
     `orchestrator_could_not_start` counts a failed spawn attempt before
     checking its budget, so its give-up call is itself the
     `SPAWN_RETRY_BUDGET`th attempt, but `relaunch_wedged` gives up *in
     place of* relaunching on the call where its own count reaches the
     budget, so only `SPAWN_RETRY_BUDGET - 1` relaunches were actually
     performed by then — the summary reports that, not the budget itself.
     This is deliberately not the `disconnected` flag the liveness
     sweep's `retire_disconnected` also raises on every ordinary crash,
     well before any budget is spent and while a relaunch may still be
     coming — reading that flag as a give-up was exactly the false
     positive an earlier round of this list raised. One task failed on
     the daemon's own descriptor-limit words is `resource`, grouped into
     one item naming every such task — one machine out of descriptors,
     not one item per task it happened to fail. Every other one is its
     own `unknown` item, naming that task alone and carrying its own
     ended reason as the summary, rather than guessed into a group with
     no reliable evidence it shares a cause with another. Both items'
     `since` — the grouped item's own oldest one, and the unknown item's
     — reads the failing transition's own `created_at`
     (`Store::latest_transition_to_with_time`), never `Task::updated_at`:
     an edit to a still-failed task's description or staffing moves the
     latter, which would otherwise reset the item's reported waiting time
     for a reason that has nothing to do with the failure itself.
   - every goal still `planning` or `active` whose orchestrator has been
     given up on (`Goal::orchestrator_given_up_at`) — one cancelled or
     completed no longer needs the recovery its mark names, whatever that
     mark still says. `orchestrator_given_up_items` raises this beside a
     failed task's own item rather than instead of it: a task that failed
     for its own reason and an orchestrator that will not start are two
     different problems with two different required actions, and the
     task's own item carries no "resume it" action for this one to be
     dropped in favour of — so both are raised, each naming its own
     subject (the task, the goal) and its own action. The item names the
     goal and targets the alarm session the give-up itself is raised on
     (or the goal's last orchestrator session where none still carries
     it), and its summary carries the spawn-retry budget spent
     (`SPAWN_RETRY_BUDGET`) and the alarm session's own last
     `session.error`, where it reported one (`last_session_error`).
   - every pull request whose reviewer session has been given up on
     (`PullRequestRow::reviewer_given_up_at`, the same kind of mark,
     stamped by `scheduler::pull_requests::start_pull_request_session`
     once *its* spawn-retry budget runs out, or by the same watchdog
     exhaustion the orchestrator's own mark reads, and cleared the same
     way — on a resume/spawn that succeeds, or on a live session's own
     `last_activity_at` moving past the mark, in `review_pass`, never
     merely because the session is found live — *or* the moment the
     request no longer wants a reviewer session at all —
     `scheduler::pull_requests::end_review`, reached once `wants_session`
     answers `false` for any reason: closed, no longer asking, back to
     draft, or the repository's own review pin gone — so a request that
     stopped asking does not carry a stale blocker for work nothing is
     trying any more): a reviewer session sits on no task either, and
     carries the same `disconnected` false-positive risk the
     orchestrator's own mark exists to avoid. `reviewer_given_up_items`
     gives it its own item, targeting the request itself, with the same
     budget-and-last-error summary the orchestrator's own item carries.
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
     already failed, and only on a *conclusive* rejection:
     `ForgeClient::confirmed_signed_out` reads the same account call
     `whoami` already makes (`api user`, a real round trip to the forge,
     not `auth status`'s own composite of config and SSO checks that can
     fail before ever reaching the forge) and answers `true` only once
     both `Refusal::ran()` — the CLI actually reached an exit code,
     however it answered — and `Refusal::is_unauthorized()` — positive
     evidence of exactly a credential rejection, not merely a forbidden
     response — hold: the forge's own answer carried `HTTP 401` (the API
     never answers it for anything else) or the fixed words `Bad
     credentials` (GitHub's own phrase for a rejected or revoked token).
     A bare `HTTP 403` is deliberately *not* read as one: GitHub answers
     the same `HTTP 403` for primary and secondary rate limiting as it
     does for a rejected credential, with otherwise valid credentials
     (GitHub's own REST rate-limit documentation), and for a restriction
     an already-authenticated, already-valid token can still be caught
     by — an IP allow list, SAML enforcement (GitHub's own network access
     restriction documentation) — neither of which a sign-in answers for.
     A 403 naming either is read as inconclusive, never as a confirmed
     sign-out, so `is_unauthorized` requires `HTTP 401` or `Bad
     credentials` by name rather than inferring anything from a 403's
     status alone. A missing binary, a spawn or write failure, or a timeout
     never ran at all; a network failure or a server error of the forge's
     own ran, but named no credential rejection: both answer `false`, and
     the marker is never written, leaving `configuration`'s own cause, or
     nothing, to say why the fetch failed instead. `access` is reserved
     for the forge itself having conclusively said no. Checking an existing account
     call after a failure that already happened is not a new watch over
     the forge — it is one answer, read once, to the one question "did
     the forge reject these credentials" — so it stays inside the "no new
     monitoring service" line rule 4's `configuration` cause already
     holds for a fetch error with no such confirmation.
5. `access` is produced by the recovery path above the moment the daemon's
   own reactive sign-in check confirms it; naming one from a forge CLI's
   free-text error alone, or from the advisory failure classifier (024),
   would still be exactly the speculative classification this list does
   not raise, and recovery does neither.
6. A client reads this list live by invalidating it on the same SSE events
   that already carry its evidence — `session_updated`, `task_updated`,
   `repository_updated`, `repository_deleted`, `goal_deleted`,
   `goal_updated` (the orchestrator's own confirmed-turn and give-up
   marks ride on it) and `pull_requests_changed` (the reviewer's own
   give-up mark rides on it) — rather than a dedicated event: the
   recovery producer's reads are cheap enough that a fresh
   `GET /v1/attention` on each of those is simpler than patching a
   derived list by hand. Every store write to one of these marks
   publishes the same fat event the entity's own writes already do
   (`Store::publish_goal_update`, `Change::PullRequestsChanged`) before
   returning, so a client that reacted to the status change the mark
   rode in on and still saw nothing yet gets a second event, right
   behind it, once the mark itself has actually committed.
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
    changes to add one. `agent_requests` is registered already, answering
    an empty list, so the route a later task's producer and client
    migration need already compiles and runs; the question eligibility
    rules themselves, and migrating the client behavior they already
    carry onto this route, are out of this spec's scope (see Scope).
    `pull_requests` is the second complete producer, below.

## The pull-request producer

11. `crate::attention::pull_requests` raises two items, both read off a
    request's last fetch (`crate::forge::live::of_row`) rather than from a
    guess: a review request nobody is assigned to, and a request a
    babysitting task has claimed is ready to merge, confirmed against the
    forge's own evidence for the current head. Neither applies to a
    request that is closed, merged, back in draft, or that the last fetch
    has not read yet (`of_row` answers `None`): missing evidence withholds
    both items the same way it withholds a recovery one.
12. A review request item (`AttentionCause::Configuration`) is raised for
    a `reviewer` row that still asks for the user's review
    (`review_requested`) where neither the repository's own
    `review_model` nor one asked directly on the row
    (`PullRequestRow::review_model`) names a model: `wants_session`
    (029) answers `false` for exactly this reason, and nothing automatic
    is ever going to start a session for it, so a human still has
    something to start by hand. A request `recovery`'s own
    `reviewer_given_up_items` already carries
    (`PullRequestRow::reviewer_given_up_at`) raises nothing more here: that
    is the same underlying request as a different kind of "nobody is
    handling it", and a second item would only repeat the first's fix.
    Starting a session — on the repository's own pin, or by asking
    directly on the row through `PUT
    .../pull-requests/{number}/ariadne-review` (029 rule 10, open to a
    review request on an unpinned repository the same as to a request of
    the user's own) — clears the item the moment `wants_session` answers
    `true` for it; a reviewer taking the row over, the same route's
    `asked: false`, or the repository gaining a pin removes it the same
    way. The item's `since` is the row's own `created_at`: a review
    request gets its row the moment the first fetch finds it (029 rule 1),
    whether or not it is pinned, so that moment is also the moment nobody
    was yet assigned to it.
13. A readiness item (`AttentionCause::Unknown`) is raised for an `author`
    row a task keeps (`origin_task_id`) once every one of three pieces of
    the forge's own evidence for the request's current head holds
    together: `review_decision == "approved"` (a current approval, not an
    Ariadne review's own summary, which the forge never counts as one —
    029 rule 14 refuses an approval from any review route this daemon
    runs), `checks == "success"` (every check green) and `mergeable ==
    "clean"` (rule 14 — the forge's own confirmation the head can be
    merged now, not merely inferred from the checks rollup). Beside
    those three, the comment evidence itself must hold two ways:
    - every *resolvable* review thread must actually be resolved on the
      forge, whoever opened it: read by `kind == "review_comment"`
      rather than by who posted it (`from_review`), so a human's own
      unresolved finding blocks exactly as an Ariadne one does, and a
      review's own summary — posted as a plain `issue_comment`, never a
      resolvable thread at all — is never read as one that blocks
      forever. A reply alone answers a thread for routing purposes
      without resolving it; only an explicit resolve on the forge
      clears it here;
    - the conversation side — read live off `forge::live::waiting_threads`
      on this item's own call rather than off the row's own cached
      `unanswered_comments`, which still counts every comment the
      login's own side has not answered (029 rule 13's own "news"
      reading) — must hold no open thread once this item first excludes
      the one comment that reading would otherwise block forever: the
      review's own summary, the single `issue_comment` the login itself
      posts to the conversation thread (`forge/pulls.rs::CONVERSATION`).
      A summary carries nothing to answer, and the babysitting skill
      never replies to a comment that asks for no change, so this item
      must never wait on an invented reply to clear it; a genuine human
      comment sharing that same thread still blocks, excluded by
      neither its thread nor its login.

    The comment evidence itself must also be both successfully
    refreshed (`Live::evidence_ok`, set `false` by a failed fetch and
    left `false` until the next one succeeds, whether that failure was
    a detail fetch, the outer list call, an initial listless lookup, or
    a route-level read off a single request, and whether or not the
    head moved too) and current for this head
    (`Live::Details::head_sha == pull.head_sha`): either short, a failed
    or stale refresh must never be read as "no open comment"
    (`forge::live::LivePulls::mark_evidence_failed`,
    `::set_pull_evidence_failed`). The babysitting task's own `ready`
    report (`POST /pull-requests/{id}/report`, 029 rule 15) is read
    beside all of this as a further condition, never in their place,
    and is itself bound to the head the agent actually read: the report
    names the `head_sha` it confirmed readiness on, and the item
    withholds unless that bound head (`PullRequestRow::ready_head_sha`)
    still matches the request's current head — `ready` alone, or the
    forge conditions without a report naming the current head, raises
    nothing. A request no task keeps — asked for ad hoc, through the
    same report route a reviewer session also answers to (029 rule 11)
    — raises nothing either, whatever its `ready` flag says: only the
    babysitting task's own claim is read, never a bare flag on a row
    nothing manages (029, "Only the babysitter raises readiness
    attention for a request it manages"). Every field above is the live
    state of the request's current head, read fresh on every call to
    this producer rather than latched anywhere: a later commit, a
    reopened comment, an approval the forge no longer counts (dismissed,
    or superseded by a new push), or a failed evidence refresh drops the
    item on the very next read, with nothing of its own to invalidate.
    The item's `since` is `PullRequestRow::ready_confirmed_at`, stamped
    the moment `ready` itself last moved from `false` to `true`
    (`Store::set_pull_request_ready`) and cleared the moment it moves back
    — not `updated_at`, which a fetch bumps on every poll whether or not
    anything about readiness changed, the same trap rule 4's own `since`
    values avoid for a task's `failed` transition. Every path that
    invalidates the comment evidence — the poll's own outer list call,
    its initial listless lookup, its detail fetch, and a direct,
    route-level read off a single request outside the poll's own cycle
    (`crates/ariadne-daemon/src/http/pull_requests.rs::read_now`) —
    publishes `pull_requests_changed` on the transition, so a client
    watching the stream re-reads this item rather than holding a
    withdrawn one, or a restored one, on an older read
    (`kept_requests.rs::an_outer_list_failure_withdraws_the_rows_evidence`,
    `::a_direct_read_failure_withdraws_the_rows_evidence`,
    `::a_route_level_read_failure_withdraws_the_readiness_item_and_its_recovery_restores_it`,
    `pull_requests.rs::an_outer_list_failure_publishes_pull_requests_changed`,
    `::a_route_level_read_failure_withdraws_the_rows_evidence`).
14. `mergeable` is read off the forge's own mergeability for the head,
    never derived from `checks` or `review_decision`, and read by each
    state's own documented meaning rather than guessed from its name:
    GitHub's `mergeStateStatus`
    (https://docs.github.com/en/graphql/reference/enums#mergestatestatus)
    answers `clean` for `CLEAN` and, by that same documentation, for
    `HAS_HOOKS` ("mergeable with passing commit status and pre-receive
    hooks") and `UNSTABLE` ("mergeable with non-passing commit status") —
    `checks` is this producer's own gate on commit status passing, so
    reading `UNSTABLE` as `clean` here never lets a non-passing check
    through on its own; `UNKNOWN`, or no field at all, is `unknown`; every
    other named state — `BEHIND`, `BLOCKED`, `DIRTY`, `DRAFT` — is
    `blocked`, each for its own reason. GitLab's `detailed_merge_status`
    answers `clean` for `mergeable`; `unknown` for `unchecked`, `checking`,
    `preparing`, `ci_still_running`, or no field at all — the forge still
    computing it, or simply not telling us, never a guessed `clean`; every
    other named status is `blocked`. `unknown` is never read as `clean`: a
    readiness item answers for a confirmed mergeability, not an
    unevaluated one
    (`forge/github/pulls.rs::tests::mergeable_reads_every_merge_state_status_by_its_documented_meaning`,
    `::a_read_naming_no_merge_state_status_is_unknown`,
    `forge/gitlab/pulls.rs::tests::mergeable_reads_every_detailed_merge_status_by_its_documented_meaning`,
    `::a_read_naming_no_detailed_merge_status_is_unknown`).

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
- A failed task raises nothing while its orchestrator's own turn on it is
  still running, proven end to end through the real scheduler and a stub
  agent held open on `wait_for` for exactly that window — not merely
  before delivery and after completion, which a stamp taken at hand-off
  time would also pass, but during the gap between the two
  (`attention.rs::a_failed_tasks_item_stays_suppressed_while_its_orchestrators_turn_on_it_is_still_running`).
  An unrelated turn — one carrying no `Delivery::GoalAttention` at all —
  confirms nothing when it ends, proven the same way: the confirmed-list
  column stays untouched by it
  (`attention.rs::an_unrelated_turn_confirms_nothing_it_never_carried`).
  Confirmation is read by this task's own id, never by whether the goal
  has any confirmed turn at all — a turn that confirmed a different
  failure does not answer for this one
  (`attention.rs::a_failed_tasks_item_still_waits_while_only_a_different_failure_was_confirmed`),
  and a confirmation does not survive its own task's retry: a second
  failure of the same task stamps a fresh transition id the old
  confirmation does not carry
  (`attention.rs::an_old_confirmation_does_not_answer_for_a_tasks_second_failure`).
  Editing a confirmed, still-failed task's description — metadata
  `Store::update_task` allows editing on a `failed` task — does not move
  the transition id the confirmation is keyed on, so the item survives
  the edit
  (`scheduler_attention.rs::editing_a_failed_tasks_description_does_not_lose_its_recovery_item`).
  A second failure of the same task, missed by the scheduler before any
  reconciliation pass ran in between, renders the identical
  `goal_attention` situation text as the first — title, id and `failed`
  status, nothing else — so the in-memory `goal_told` cache alone cannot
  tell them apart; the orchestrator is told of it anyway, since the
  persisted, transition-id-keyed confirmation can
  (`scheduler_attention.rs::a_second_failure_missed_between_reconciles_is_still_told`).
  A delivery lost before its own turn could confirm it — the write never
  reaching the agent's stdin, the same race a connection ending in that
  exact window leaves behind — is retried on the next pass regardless of
  what `goal_told` still says, through the session's automatic resume,
  and confirmed once the replacement's own turn actually ends, proven end
  to end through the real scheduler rather than a hand-written store poke
  (`attention.rs::a_lost_delivery_is_retried_after_resume_and_confirmed_once_its_replacement_answers`).
  An orchestrated goal with no orchestrator session yet — awaiting its
  first launch — raises nothing either, the same grace a dead one
  gets
  (`attention.rs::a_failed_task_raises_nothing_while_its_goal_awaits_its_orchestrators_first_launch`).
  A goal whose dead orchestrator has not yet been given up on still
  raises nothing, since `keep_orchestrator` may still relaunch it
  (`attention.rs::a_failed_task_raises_nothing_while_its_dead_orchestrator_has_not_been_given_up_on`);
  a mere crash is not read as that give-up either — the `disconnected`
  flag a crash alone raises answers for nothing on its own
  (`attention.rs::a_failed_task_raises_nothing_while_its_orchestrator_merely_crashed_without_giving_up`);
  and once the goal's own give-up mark is actually set, the task's own
  item raises *and* the orchestrator's own give-up raises a second,
  independent item beside it — two different problems, two different
  actions
  (`attention.rs::a_failed_task_raises_its_item_once_its_orchestrator_is_given_up_on`).
  Retrying a failed task takes its item down without losing the
  transition that recorded why it failed
  (`attention.rs::retrying_a_failed_task_removes_its_item_without_losing_the_transition`).
- A cancelled goal's orchestrator give-up raises nothing, though the mark
  itself is left as history rather than deleted
  (`attention.rs::a_cancelled_goals_orchestrator_give_up_raises_nothing`);
  a request whose review pin is gone clears its reviewer's give-up mark
  the same way `end_review` already takes its session down
  (`attention.rs::a_request_whose_review_pin_is_gone_clears_its_reviewers_give_up`).
- A taskless orchestrator given up on — a goal still planning, with no
  failed task of its own — raises its own `unknown` item end to end
  through the real scheduler's spawn-retry budget actually running out,
  and that item answers for it even though the same crash has already
  raised the generic `disconnected` flag on the alarm row
  (`scheduler_attention.rs::an_orchestrator_that_dies_the_moment_it_starts_is_given_up_on`).
  The watchdog's own exhausted-relaunch decision — a session that stays
  silent through every relaunch rather than dying outright — leaves the
  same mark for a taskless orchestrator, with no task to fail onto
  instead
  (`scheduler_attention.rs::an_orchestrator_that_wedges_after_every_relaunch_is_given_up_on_without_a_task_to_fail`),
  and for a pull request's reviewer session the same way
  (`scheduler_attention.rs::a_reviewer_session_that_wedges_after_every_relaunch_is_given_up_on_without_a_task_to_fail`).
  Both tests back their session straight into the relaunch threshold,
  skipping the flag one entirely, so the alarm session never actually
  carries `AttentionReason::Stalled` — proving the summary still reads
  "stopped answering after 2 relaunches" (`SPAWN_RETRY_BUDGET - 1`, the
  count actually performed) from the persisted `wedged` evidence rather
  than that flag (same two tests). Both also extend through later
  reconciliation passes over the same, still-wedged session: the give-up
  mark, its `since`, and the item's presentation all stay exactly as
  they were, with no flap, since no genuine recovery happened in between
  (same two tests).
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
  fails both `pr list` and `api user`
  (`attention.rs::a_forge_cli_signed_out_mid_fetch_is_confirmed_reactively_and_reaches_the_list`).
  The marker is read from whether the CLI actually ran that check, not
  merely whether it returned an error, so a missing binary or a timeout
  on the check itself never gets misread as a confirmed rejection
  (`forge/mod.rs::a_refusal_has_run_only_where_the_cli_reached_an_exit_code`,
  and `::a_cli_that_reads_no_input_is_stopped_by_the_deadline` for a real
  timeout's own `Refusal::ran()` reading `false`). A rate-limited `HTTP
  403` on that same check — GitHub's own answer for primary and
  secondary rate limiting, with otherwise valid credentials — and a
  generic forbidden `HTTP 403` naming no credential problem at all — an
  IP allow list or SAML enforcement restriction, which an already-valid
  token can still be caught by — are both read as inconclusive, never as
  a confirmed sign-out: `is_unauthorized` requires positive evidence of
  a rejected credential, not merely a forbidden status
  (`forge/mod.rs::confirmed_signed_out_is_false_on_a_rate_limited_403`,
  `::confirmed_signed_out_is_false_on_a_generic_forbidden_403`). The
  timeout test itself is held open until the configured deadline by a
  shared stub script rather than `/bin/sleep` mis-parsing the check's
  own arguments
  (`forge/mod.rs::confirmed_signed_out_is_false_on_a_timeout`).
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
- Desktop refetches the attention list on a goal's own confirmed-turn or
  give-up evidence (`goal_updated`) and on a reviewer session's own
  give-up evidence (`pull_requests_changed`)
  (`dispatch.test.ts`, "refetches the attention list on a goal's own
  confirmed-turn or give-up evidence", "refetches the attention list on a
  reviewer session's own give-up evidence"); the CLI's `--watch` treats
  both the same way
  (`attention.rs::watch_redraws_on_a_goals_or_a_pull_requests_own_recovery_evidence`).
- A review request with no pin anywhere offers a manual-start item naming
  its repository, and takes none once the row or the repository gains one;
  the same request already given up on by recovery carries only
  recovery's own item, never a second
  (`attention.rs::an_unpinned_review_request_offers_a_manual_start_item`,
  `::a_review_request_pinned_on_its_repository_offers_no_manual_start_item`,
  `::a_review_request_recovery_has_given_up_on_carries_no_second_item`).
  `ariadne pr review` and the desktop panel's Start review both reach the
  same route on such a request, the same way they do on a request of the
  user's own
  (`pull_request_reviews.rs::asking_needs_a_model_and_takes_an_unpinned_review_request`,
  `pull-request-panel.test.tsx::offers a manual Start review on a request that asks for my review when the repository pins none`).
  The manual ask itself survives a scheduler pass that runs before the
  next forge fetch could ever confirm the request still asks: the live
  cache's own `review_requested` is written `true` by the ask route
  itself for exactly this row, not merely held over from an earlier
  fetch, so `scheduler::pull_requests::end_review` never reads the
  request as having stopped asking and deletes it out from under the
  reviewer session the same call just started
  (`pull_request_reviews.rs::asking_needs_a_model_and_takes_an_unpinned_review_request`).
- A babysat request's readiness item answers only once approval, no open
  conversation-side comment, every *resolvable* review thread actually
  resolved (not merely answered, and regardless of who opened it), green
  checks, confirmed mergeability, a successful and current comment-
  evidence refresh, and a claim bound to the request's own current head
  all hold together; any one short of that — pending checks, an open
  conversation comment, a human or Ariadne review finding that is
  answered but not resolved, mergeability the forge has not confirmed, a
  failed or stale comment-evidence refresh, a disabled integration, or a
  claim a later push has already passed — withholds it even where the
  task's own `ready` claims it, and a request no task keeps raises
  nothing whatever its `ready` says; a review's own summary comment never
  blocks the item on its own, and a completed review whose summary and
  every finding are answered or resolved raises it
  (`attention.rs::a_ready_request_the_forges_own_evidence_backs_up_raises_a_readiness_item`,
  `::a_ready_claim_with_resolved_comments_but_no_approval_raises_no_readiness_item`,
  `::a_ready_claim_with_pending_checks_raises_no_readiness_item`,
  `::a_ready_claim_with_an_open_review_comment_raises_no_readiness_item`,
  `::a_ready_claim_with_an_ariadne_finding_answered_but_unresolved_raises_no_readiness_item`,
  `::a_ready_claim_with_a_human_finding_answered_but_unresolved_raises_no_readiness_item`,
  `::a_completed_review_with_its_real_summary_and_a_resolved_finding_raises_a_readiness_item`,
  `::a_ready_claim_with_a_resolved_review_thread_raises_a_readiness_item`,
  `::a_ready_claim_with_unconfirmed_mergeability_raises_no_readiness_item`,
  `::a_ready_claim_on_a_disabled_integration_raises_no_readiness_item`,
  `::a_push_past_the_ready_head_withdraws_the_item_until_reconfirmed`,
  `::comment_evidence_behind_the_live_head_raises_no_readiness_item`,
  `::a_failed_refresh_at_the_same_head_raises_no_readiness_item`,
  `::a_ready_claim_on_a_request_no_task_keeps_raises_no_readiness_item`).
  Only the babysitting task's own column agent may report a task-kept
  request ready: a reviewer session that also answers for the same row —
  an Ariadne self-review asked on a request a task also keeps — gets 403
  on `ready`, though its own `reviewed_sha` still goes through
  (`kept_requests.rs::the_pr_agent_replies_and_reports_and_no_other_session_may`).
- Neither a posted review nor a ready report raises `waiting_user` on the
  session any more, so neither shows a row, a toast or a badge increment
  of its own on either client, even once the agent that carried the flag
  is relaunched
  (`pull_request_reviews.rs::a_reviewed_sha_is_stored_and_raises_no_waiting_user`,
  `kept_requests.rs::the_pr_agent_replies_and_reports_and_no_other_session_may`,
  `workflow_pull_request.rs::the_pr_column_opens_the_request_once_and_keeps_it`,
  `attention/board.rs::a_pull_request_sessions_own_waiting_user_raises_no_row_of_its_own_on_this_board`,
  `scheduler_attention.rs::a_pr_agent_whose_open_request_reads_ready_raises_no_waiting_user_on_its_restart`).
- A mergeability-only transition still publishes `pull_requests_changed`,
  both directions, and an unchanged fetch after it still raises nothing:
  (`pull_requests.rs::a_mergeability_only_transition_publishes_pull_requests_changed`).
- A `pull_request` producer item renders through the shared recovery
  rendering extension point, not the task or session fallback: a toast
  titles an unassigned review request "Review needed" and a readiness
  item "Ready to merge", naming the request, its repository and its
  required action; a session with no producer item behind it — a
  completed automated review's own bare `waiting_user` chief among them —
  raises neither a notification nor a badge; and repeated reads of the
  same item raise one toast, a withdrawal raises none, and a later,
  different readiness transition on the same request raises its own
  (`attention-alerts.test.tsx::raises a toast titled for an unassigned review request, naming the request and its action`,
  `::titles a readiness item as ready to merge`,
  `::raises no notification or badge for a session with no recovery item behind it`,
  `::raises one toast per readiness transition, none for a repeat, and none for a withdrawal`).

## Known gap

- `agent_requests` is a registered producer module that answers empty:
  the question eligibility rules this route was built for, and migrating
  the client behavior that already covers it onto it, are a later task's.

## Sources

`crates/ariadne-api/src/attention.rs`, `crates/ariadne-daemon/src/attention/`
(`mod`, `recovery` — including `orchestrator_has_answered_for`,
`access_items`, `orchestrator_given_up_items`, `reviewer_given_up_items`,
`last_session_error` — `agent_requests`, `pull_requests` —
`review_start_item`, `readiness_item`),
`crates/ariadne-daemon/src/http/attention.rs`, `crates/ariadne-daemon/src/acp.rs`
(`Delivery::GoalAttention`, `send_goal_attention`, `claim_message`,
`serve_with_input`'s confirmation of a completed `GoalAttention` turn),
`crates/ariadne-daemon/src/scheduler/auto_switch.rs` (`recovery_block`,
`switch_chain`, `switch_target`), `crates/ariadne-daemon/src/scheduler/goals.rs`
(`tell_orchestrator`, `hand_goal_attention`, `keep_orchestrator`,
`orchestrator_could_not_start`), `crates/ariadne-daemon/src/scheduler/pull_requests.rs`
(`start_pull_request_session`, `review_pass`, `end_review`),
`crates/ariadne-daemon/src/scheduler/quiet.rs` (`relaunch_wedged`'s taskless
give-up branch), `crates/ariadne-store/src/goals.rs`
(`confirm_goal_orchestrator_answered`, `set_goal_orchestrator_given_up`,
`clear_goal_orchestrator_given_up`, `Goal::orchestrator_answered_failed_task_ids`,
`Goal::orchestrator_given_up_at`), `crates/ariadne-store/src/pull_requests.rs`
(`set_pull_request_reviewer_given_up`, `clear_pull_request_reviewer_given_up`,
`PullRequestRow::reviewer_given_up_at`, `set_pull_request_ready`,
`PullRequestRow::ready_confirmed_at`), `crates/ariadne-daemon/src/forge/mod.rs`
(`ForgeClient::confirmed_signed_out`, `Refusal::ran`, `Refusal::is_unauthorized`),
`crates/ariadne-daemon/src/forge/poll.rs` (`FORGE_SIGNED_OUT`, `wants_session`),
`crates/ariadne-daemon/src/forge/pulls.rs` (`ForgePullRequest::mergeable`),
`crates/ariadne-daemon/src/forge/github/pulls.rs`,
`crates/ariadne-daemon/src/forge/gitlab/pulls.rs`,
`crates/ariadne-daemon/src/http/pull_requests.rs` (`ask_review`, `report`),
`crates/ariadne-cli/src/commands/attention.rs` (including `relevant`),
`crates/ariadne-cli/src/commands/attention/board.rs`,
`ui/src/features/goals/attention.ts`, `ui/src/features/goals/attention-alerts.tsx`,
`ui/src/features/goals/attention-strip.tsx`, `ui/src/events/dispatch.ts`,
`ui/src/features/sessions/session-display.tsx`,
`ui/src/features/pull-requests/pull-request-panel.tsx`.
