---
id: pull-requests
status: current
updated: 2026-10-07
areas: [store, api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/src/forge/mod.rs
  - crates/ariadne-daemon/tests/it/pull_requests.rs
  - crates/ariadne-daemon/tests/it/landing_lifecycle.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/pr.rs
  - ui/src/features/pull-requests/pull-requests-page.test.tsx
  - ui/src/features/pull-requests/pull-request-panel.test.tsx
  - ui/src/routes/paths.test.ts
---

# Pull requests

The ledger holds requests opened by a task, found on the forge, or added by a user.

## Scope

In: request identity, persistence, forge reads, fetch modes, HTTP, events, CLI, and desktop views.

Out: session attachment, comment detail fetching, and webhook delivery.
Forge detection and integration settings belong to [025](025-forge-integration.md).
Task completion belongs to [005](005-how-a-task-ends.md).

## Behavior

1. A request has one identity: `(repository_id, number)`.
   The database enforces this pair as unique.
   `PullRequestRef::parse(url, integration)` resolves every birth before `Store::upsert_pull_request` writes it.
   The parser accepts HTTP or HTTPS, trailing slashes, queries, and fragments.
   Host, owner, and repository name comparisons ignore case.
   GitHub uses `/pull/<number>`.
   GitLab accepts `/merge_requests/<number>` and `/-/merge_requests/<number>`, including subgroup paths.
   A different repository or host does not match.
2. The `pull_requests` table holds the complete `PullRequestDto`.
   Its fields are `id`, `repository_id`, `number`, `url`, `title`, `author_login`, `role`,
   `tracked_by`, `state`, `draft`, `head_branch`, `head_sha`, `head_repo`, `base_branch`,
   `checks`, `review_decision`, `unanswered_comments`, `ready`, `origin_task_id`,
   `opened_at`, `last_seen_at`, `created_at`, and `updated_at`.
   `url` is the forge's canonical URL; `head_repo` is a nullable clone URL.
   `state` is `open`, `merged`, or `closed`.
   `checks` is `pending`, `success`, `failure`, or `none`.
   `review_decision` is `approved`, `changes_requested`, `review_required`, or `none`.
3. `role` is `author` when `author_login` matches the integration login, ignoring case.
   Otherwise, it is `reviewer`.
   An upsert keeps the existing ID, creation time, user tracking, and a non-null origin task.
   It preserves `ready` and `unanswered_comments`, which later detail work owns.
   New rows initialize both fields to zero.
4. An enabled repository's `open_pull_request` handler records the request immediately, with its task as origin (005).
   It reads the request by number after the daemon opens it.
   An off integration creates no ledger row, but the task keeps its URL.
   A retry uses that saved URL and can complete a failed ledger read without opening another request.
5. A repository fetch lists every open request authored by the login or requesting its review.
   It deduplicates the lists and records the requests with `tracked_by = forge`.
   It reads every user-tracked row individually, including rows absent from both lists.
   It also reads open forge-tracked rows that disappeared from the lists.
   Only an individual forge read changes such a row to `merged` or `closed`.
   Absence from a list never closes a user-tracked row.
6. `ForgePoll::wake(repository_id)` schedules one fetch immediately.
   Each repository has one worker, so fetches of that repository never overlap.
   Wakes during a fetch coalesce into one following fetch.
   `set_mode(repository_id, Mode::Timer)` enables the fallback timer.
   `Timeouts::forge_poll` defaults to 60 seconds.
   `Mode::WakeOnly` has no timer fetches; only explicit wakes fetch.
   Enabled integrations start with one immediate fetch.
   [027](027-webhooks-and-tunnel.md) chooses `WakeOnly` for a live hook and `Timer` otherwise.
   Enabling starts one immediate fetch; disabling cancels its worker and closes its open rows.
   Daemon startup fetches every enabled integration once.
   The timer is the last resort. A live webhook in task 027 switches its repository to `WakeOnly`.
7. `after_fetch(repository_id, changed_rows)` is the handoff for later detail fetching and news routing.
   `unanswered_comments` counts unresolved threads whose last comment is by a login other than the integration login.
   A thread answered last by that login counts zero. A resolved thread counts zero.
   The later detail fetch computes this count. The list fetch preserves its stored value.
8. Each first insertion publishes `pull_request_created`.
   Later upserts publish `pull_request_updated`; removal publishes `pull_request_deleted`.
   All three carry the complete DTO through the event bus (012).

## HTTP

All routes appear in OpenAPI, under the rules of [012](012-http-api-events-and-usage.md).

| Route | Behavior |
| --- | --- |
| `GET /v1/pull-requests?repo=&role=&state=` | Filter the ledger. State defaults to `open`; `all` includes terminal rows. |
| `GET /v1/pull-requests/{id}` | Read the complete DTO. |
| `GET /v1/repositories/{id}/pull-requests/search?q=` | Search open requests live by number, title, or author. Each match includes its role and `tracked` flag. |
| `POST /v1/pull-requests` | Accept `{repository_id, number}` or `{url}`. Read and upsert with user tracking. Return 201 on insertion, otherwise 200. |
| `DELETE /v1/pull-requests/{id}` | Remove a user-tracked row. Refuse a forge-tracked row with 409. |
| `POST /v1/pull-requests/refresh?repo=` | Wake one repository or every enabled repository. Return 202. |

An explicit repository with its integration off answers 409.
A URL matching no enabled integration answers 404.
A search result uses the same URL parser as an insertion.

## CLI and desktop

The command and screen follow [014](014-command-line-interface.md) and [015](015-desktop-app.md).

- `pr ls [--repo <id|path>] [--role author|reviewer] [--all] [--watch]` lists the ledger.
- `pr inspect <id>` prints every DTO field.
- `pr search --repo <id|path> <query>` searches the forge.
- `pr add <url>` tracks a request by hand.
- `pr rm <id>` removes a user-tracked request.
- `pr refresh [--repo <id|path>]` requests a fetch.

The navigation item opens `#/pull-requests`, titled "Pull requests".
Filters select repository, role, and whether terminal rows are included.
Columns show repository, number and linked title, role, tracking source, draft, checks, review decision, unanswered comments, and update time.
The header offers Refresh and Add pull request.
The Add dialog selects enabled repositories, searches live, and shows each match's author, role, and Add button.
It also accepts a URL, matching the CLI action.
A row opens the floating pane through `?pr=<id>` and `paths.pullRequest`.
The panel shows every field printed by `pr inspect`.
Both the row and panel offer Remove for user-tracked requests.
The three events update `pullRequests.detail` and invalidate `pullRequests.list`; no screen polls.

## Acceptance criteria

- URL variants share one identity and foreign repositories fail:
  `forge/mod.rs::tests::pull_request_urls_share_the_repository_and_number_only`.
- All six orders of the three births keep one row, one creation event, user tracking, and origin:
  `pull_requests.rs::all_three_births_keep_one_identity_in_every_order`.
- An off integration keeps the task URL without creating a row:
  `landing_lifecycle.rs::opening_a_pull_request_runs_the_forge_cli_once_and_ends_the_task`.
- Fetches assign roles, update existing rows, mark tracked search matches, preserve absent user rows, and filter terminal rows:
  `pull_requests.rs::fetch_tracks_roles_once_reads_missing_rows_and_closes_only_from_the_forge`.
- Adding URL variants returns 201 then 200 and supports user removal:
  `pull_requests.rs::adding_a_url_variant_keeps_one_row_and_user_tracking`.
- A user removal during an individual read stays removed:
  `pull_requests.rs::removing_a_user_row_during_its_read_does_not_recreate_it`.
- Disabling cancels an active fetch without reopening rows:
  `pull_requests.rs::disabling_during_a_fetch_cancels_it_without_reopening_rows`.
- Timer, WakeOnly, enable, refresh, disable, and disabled refusals work:
  `pull_requests.rs::timer_wake_only_and_disable_control_repository_fetches`.
- Wakes coalesce without concurrent fetches:
  `pull_requests.rs::wakes_during_a_fetch_coalesce_into_one_following_fetch`.
- Startup fetches enabled repositories and individually reads missing forge rows:
  `pull_requests.rs::startup_fetches_enabled_repositories_and_reads_a_missing_forge_row_as_merged`.
- GitLab reads fork clone URLs, checks, approvals, and reviewer roles; search matches number, author, or title:
  `pull_requests.rs::gitlab_requests_keep_fork_checks_review_and_search_by_author_or_number`.
- Store updates preserve user tracking and detail-owned fields, and refuse forge-row deletion:
  `store.rs::pull_requests_keep_identity_user_tracking_and_detail_fields`.
- The additive migration preserves existing repository data and a restorable backup:
  `store.rs::pull_request_migration_preserves_existing_rows_and_a_recoverable_backup`.
- CLI actions use the ledger routes and preserve search text:
  `pr.rs::tests::pr_commands_use_the_ledger_routes_and_preserve_search_text`.
- The screen renders rows, applies events, opens the pane, searches, and adds:
  `pull-requests-page.test.tsx`.
- The panel shows every inspect field and removes a user row:
  `pull-request-panel.test.tsx`.
- Opening a task or session replaces the request panel and preserves list filters:
  `paths.test.ts::pull request panel navigation`.

## Sources

`crates/ariadne-store/migrations/0007_pull_requests.sql`,
`crates/ariadne-store/src/pull_requests.rs`, `crates/ariadne-api/src/pull_requests.rs`,
`crates/ariadne-daemon/src/forge/`, `crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/http/landing.rs`, `crates/ariadne-daemon/src/bus.rs`,
`crates/ariadne-cli/src/commands/pr.rs`, `ui/src/features/pull-requests/`.
