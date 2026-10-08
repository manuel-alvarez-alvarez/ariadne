---
id: pull-requests
status: current
updated: 2026-10-08
areas: [store, api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/src/forge/mod.rs
  - crates/ariadne-daemon/src/forge/news.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/tests/it/pull_requests.rs
  - crates/ariadne-daemon/tests/it/kept_requests.rs
  - crates/ariadne-daemon/tests/it/scheduler_attention.rs
  - crates/ariadne-daemon/tests/it/adapters.rs
  - crates/ariadne-daemon/tests/it/session_list.rs
  - crates/ariadne-daemon/tests/it/landing_lifecycle.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/pr.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/commands/session.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/src/commands/attention/board.rs
  - crates/ariadne-store/src/defaults.rs
  - ui/src/features/pull-requests/pull-requests-page.test.tsx
  - ui/src/features/forge/forge-page.test.tsx
  - ui/src/components/app-shell.test.tsx
  - ui/src/features/pull-requests/pull-request-panel.test.tsx
  - ui/src/routes/paths.test.ts
  - ui/src/features/sessions/sessions-page.test.tsx
  - ui/src/features/sessions/session-panel.test.tsx
  - ui/src/features/goals/attention.test.tsx
  - ui/src/features/goals/attention-strip.test.tsx
  - ui/src/events/dispatch.test.ts
---

# Pull requests

The ledger holds every open request of an enabled repository, found on the forge, opened by a task, or added by a user.
A request a task opened is kept by that task's author until a human merges or closes it (005): the daemon feeds that author the request's news.

## Scope

In: request identity, persistence, forge reads, fetch modes, HTTP, events, CLI, and desktop views.
Also in: the author who keeps a request, its comments and checks, the detail fetch, the news, the tools, and the cleanup.

Out: webhook delivery, and review sessions, on a request that asks for the user's review or one of theirs they asked Ariadne to review ([029](029-reviewing-a-request.md)).
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
   `opened_at`, `last_seen_at`, `created_at`, `updated_at`, `body` (the description),
   `review_requested` and `review_asked` (029).
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
   An off integration is refused before anything is opened (005 rule 6).
   A retry uses the task's saved URL and can complete a failed ledger read without opening another request.
5. A repository fetch lists every open request of the repository, and the ones that ask for the login's review.
   It deduplicates the lists and records the requests with `tracked_by = forge`.
   A request of the login's that no task opened is recorded too, and nobody keeps it.
   Only the list of review requests marks a row as asking (029 rule 3); a row the open list alone holds never asked, and no timeline is read for it.
   It reads every user-tracked row individually, including rows absent from both lists.
   It also reads open forge-tracked rows that disappeared from the lists.
   Only an individual forge read changes such a row to `merged` or `closed`.
   Absence from a list never closes a user-tracked row.
6. `ForgePoll::wake(repository_id)` schedules one fetch immediately.
   Each repository has one worker, so fetches of that repository never overlap.
   Wakes during a fetch coalesce into one following fetch.
   `set_mode(repository_id, Mode::Timer)` enables the fallback timer.
   `Timeouts::forge_poll` defaults to 5 minutes.
   Each fetch also reads the repository's open issues, and publishes `issues_changed` with the repository id where they moved since that worker's last read (028).
   `Mode::WakeOnly` has no timer fetches; only explicit wakes fetch.
   Enabled integrations start with one immediate fetch.
   [027](027-webhooks-and-tunnel.md) chooses `WakeOnly` for a live hook and `Timer` otherwise.
   Enabling starts one immediate fetch; disabling cancels its worker and closes its open rows.
   Daemon startup fetches every enabled integration once.
   The timer is the last resort. A live webhook in task 027 switches its repository to `WakeOnly`.
7. `after_fetch` hands every row a fetch changed to the detail fetch and to the scheduler (rules 13 and 16).
   `unanswered_comments` counts unresolved threads whose last comment is by a login other than the integration login.
   A thread answered last by that login counts zero. A resolved thread counts zero.
   The detail fetch computes this count from the stored comments. The list fetch preserves its stored value.
8. Each first insertion publishes `pull_request_created`.
   Later upserts publish `pull_request_updated`; removal publishes `pull_request_deleted`.
   All three carry the complete DTO through the event bus (012).

## The author who keeps a request

9. A row with role `author` and an origin task is kept by that task's author (005).
   It gets no session of its own: the author's own session is told its news.
   The author is the picked winner's session on a task with several authors, the lone author's everywhere else.
   A row of the login's with no origin task is listed, and nobody keeps it.
10. The daemon loads the `pr-babysit` skill for every author of a task that lands by request: a task of a `pull_request` goal, or the final task of a `feature_branch` goal (017).
    It is how the author keeps the request: answer comments, fix checks, keep the branch current, and send every fix through `request_review` before it is pushed.
11. An author whose agent went away while its request is open is picked up by the task's own pass, with the keep-request briefing: push what an approved revision added, read the request, and wait for its news (005 rule 8).
    The same briefing picks up an author approved again after a revision.
    A restart that resumes the author while its open request still reads ready raises `waiting_user` again.
12. The scheduler reconciles requests on every full pass, on every request change, and on every event of a request's session (`scheduler/pull_requests.rs`).
    An idle author on an approved task with an open request waits on the forge: it is never nudged for sitting idle (009 rule 41).
    A disabled integration tells the author nothing; its task stays approved until the integration is on again.

## Comments, checks and the detail fetch

13. After a repository fetch, the daemon reads the details of every open row that has a live session or wants one, and of every open row a task opened.
    `ForgeClient::details(number)` answers the request, every comment with its thread and author, the failed checks, and whether the head is behind the base.
    GitHub reads `gh api` review comments, issue comments and reviews, thread ids and resolution through `gh api graphql`, check runs of the head, and a base compare.
    Every list is read with `--paginate`, the review threads and the check runs included.
    GitLab reads `glab api` discussions, the jobs of the latest pipeline, and the diverged commit count.
    One detail fetch is bounded by `Timeouts::forge_details`, 30 seconds by default.
14. `pull_request_comments` holds each comment, unique on `(pull_request_id, forge_id)`.
    Its fields are `id`, `forge_id`, `thread_id`, `kind`, `author_login`, `author_is_bot`, `body`, `path`, `line`, `in_reply_to`, `created_at`, `fetched_at`, `answered`, `resolved` and `told_at`.
    `kind` is `review_comment`, `issue_comment` or `review`. A review with no body is no comment.
    Comments on the conversation and review bodies share one thread, `conversation`.
    `answered` holds where a later comment in the thread is by the integration login.
    A comment read again keeps its row, its id and its `told_at`.
15. `pull_requests` gains `failed_checks`, a JSON list of name, URL and conclusion, and `behind_base`.
    The DTO carries both, and `session_id`: the newest author session of the origin task for a row a task opened, else the newest review session on the request.

## The news

16. `forge/news.rs` computes what the session has not been told: comments by another login with `told_at` NULL in a thread nobody answered or resolved, checks that turned to failure, a rolled-up check state that changed, `behind_base` that turned true, a review decision that changed, and a state that turned `merged` or `closed`.
    The check state is what a ready report turns on: checks that end green after an approval, or go back to pending after a report, are news.
    The prompt lists each item on one line, and names each comment by its stored id.
17. The scheduler hands the news as a `session/prompt`, queued behind a running turn (009 rule 6).
    A session still `starting` gets the news once it has its briefing.
    A fetch that finds nothing new hands nothing.
18. Each change is told once (009 rule 4).
    A told comment carries `told_at`. The row keeps a told mark for the rest: the failed checks by name, `behind_base`, the check state, the review decision and the state.
    The ACP driver writes both right before the prompt goes out, as it claims an agent message (018). A prompt that never went out, a kill before it included, gives them back.
    The claim is a compare and set: it holds only while the row still carries the mark the news was computed from, and every comment it names is untold. A stale news is refused, its prompt is skipped, and the next pass computes the news again.
    News of a request still queued is not queued again.
    A check that turns green, or a head that catches up, writes the mark at once with no prompt, so its next turn is news again.

## Tools and replies

19. The author seat lists `get_pull_request`, `list_comments`, `reply_comment` and `report_pull_request` beside its task tools (013).
    Each finds the request its task opened through `GET /v1/pull-requests?task=<id>&role=author&state=all`; with none yet, it says to call `open_pull_request` first.
    No tool and no route resolves a thread: a human closes a thread.
20. A reply posts through the forge CLI: `gh api .../pulls/<n>/comments/<id>/replies` on the thread's first review comment, else `gh pr comment`; GitLab adds a note to the discussion.
    The daemon stores the reply as a comment of the integration login, marks the thread answered, and counts `unanswered_comments` again.
21. A report with `ready: true` on a change raises `waiting_user` on the author's session. `ready: false` on a change clears it. A repeat raises nothing.
    `state` writes the row.
    Comments, replies and reports are accepted from the request's own sessions alone: the author of its origin task, or its review session (029). Another session gets 403, and so does a report or reply with no session. The user reads comments freely.
22. `finish_task` of a `pull_request` task is accepted once its request's row reads `merged` (005 rule 12). A close is told to the author, which fails the task.

## Cleanup

23. A `merged` or `closed` row is told to its author like any other news; the author finishes or fails the task.
    Once the task is over, what is left of the request is taken down: any session or worktree an earlier release started on it, and, on a merge whose head is a goal branch, that goal branch, local and remote.
    The task's own cleanup took its worktree and its branch.
    `pull_requests.cleaned_at` records the cleanup, so a restarted daemon owes the same. A deletion that fails keeps the cleanup pending: the next change of the request tries it again, and the tick after 60 seconds.
    A row born ended, and a row that ended before this release, owes no cleanup. A row that opens again owes it once more.
24. A head branch that is a `goal_repositories.goal_branch` is the goal's.
    A `merged` row deletes it, local and remote. A `closed` row deletes neither.
25. Disabling the integration closes its open rows that no task opened (rule 6), which ends their review sessions and removes their worktrees. Nobody merged or closed anything, so it tells nothing and deletes no branch.
    A row a task opened stays open, and the next fetch with the integration on reads it again.
    Removing a user-tracked row takes its sessions and worktree down first, and no branch.

## HTTP

All routes appear in OpenAPI, under the rules of [012](012-http-api-events-and-usage.md).

| Route | Behavior |
| --- | --- |
| `GET /v1/pull-requests?repo=&role=&task=&requested=&state=` | Filter the ledger: by repository, role, origin task, and whether a row asks for the user's review. State defaults to `open`; `all` includes terminal rows. |
| `GET /v1/pull-requests/{id}` | Read the complete DTO. |
| `GET /v1/repositories/{id}/pull-requests/search?q=` | Search open requests live by number, title, or author, the user's own left out. Each match includes its `tracked` flag. |
| `POST /v1/pull-requests` | Accept `{repository_id, number}` or `{url}`. Read and upsert with user tracking. Return 201 on insertion, otherwise 200. A request of the user's own is refused with 409 `pull_request_is_yours`: the fetch lists it. |
| `PUT /v1/pull-requests/{id}/ariadne-review` | Take `{asked}`: ask Ariadne to review a request of the user's own, or stop asking (029). |
| `DELETE /v1/pull-requests/{id}` | Remove a user-tracked row. Refuse a forge-tracked row with 409. |
| `POST /v1/pull-requests/refresh?repo=` | Wake one repository or every enabled repository. Return 202. |
| `GET /v1/pull-requests/{id}/comments?unanswered_only=` | List the stored comments, or the threads that wait on the login. |
| `POST /v1/pull-requests/{id}/comments/{comment_id}/reply` | Post a reply through the forge CLI and store it. Return 201. |
| `POST /v1/pull-requests/{id}/report` | Take `ready` and `state` from the request's own session. |
| `GET /v1/sessions?pull_request=` | List the sessions of one request (012). |

An explicit repository with its integration off answers 409.
A URL matching no enabled integration answers 404.
A search result uses the same URL parser as an insertion.

## CLI and desktop

The command and screen follow [014](014-command-line-interface.md) and [015](015-desktop-app.md).

- `pr ls [--repo <id|path>] [--mine | --review-requests] [--all] [--watch]` lists every open request, the user's own with `--mine` (`role=author`), or the ones that ask for their review with `--review-requests` (`role=reviewer&requested=true`); the two do not combine. Its author column reads "you" on a request of the user's.
- `pr inspect <id>` prints every DTO field.
- `pr search --repo <id|path> <query>` searches the forge.
- `pr add <url>` tracks a request by hand.
- `pr rm <id>` removes a user-tracked request.
- `pr refresh [--repo <id|path>]` requests a fetch.

The sidebar's Forge entry opens the Forge screen, titled "Forge", whose two tabs are Pull requests (`#/forge/pull-requests`, where `#/forge` lands) and Issues (028).
Pull requests lists the open rows alone. A three-way choice narrows them: All (every open request), Mine (`role=author`), and Review requests (`role=reviewer&requested=true`).
A text box narrows the rows to the words of their number, title, description, author or head branch, and a repository picker to one enabled repository. Every filter is kept in the URL.
A row shows the linked number and title with a Draft or Ariadne reviewing pill, and under it the repository, "by you" or the author, the head and base branches and "added by hand" for a user row; then pills for the checks and the review decision, the unanswered comments, a link to the session, and the update age.
A row holds no button: a click on it, or Enter on it, opens the request's panel.
The header offers Refresh. The screen adds no request by hand: `pr add` is the CLI's alone.
A row opens the floating pane through `?pr=<id>` and `paths.pullRequest`.
The panel's header carries the number and title, pills for the state, the checks, the review decision and an Ariadne review, the id, and when it opened and moved.
Under it are the facts by name — a link to the request on the forge, the repository, the author, the branches, the head, the unanswered comments, the base, how it is tracked, the task that keeps it and each failed check linked to its run — then two tabs: Description, the body rendered as Markdown, and Sessions, the request's review sessions and the task's author.
Picking a session drills into it in the panel, as `?session=` beside `?pr=`, with a way back to the request.
The panel offers Start review and Stop review on a request of the user's own (029), and Remove on a user-tracked one.
The three events update `pullRequests.detail` and invalidate `pullRequests.list`; no screen polls.
A row and the panel link to the request's session. The panel shows the failed checks and `behind_base`.
A session with a `pull_request_id` shows the request's title and a link to its URL in the sessions table Context column and the session panel.
`ariadne session ls` titles such a session with the request's title and URL. `session inspect` prints its `pull request`.

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
- CLI actions use the ledger routes and preserve search text, and `pr ls` asks every open request, mine, or the review requests:
  `pr.rs::tests::pr_commands_use_the_ledger_routes_and_preserve_search_text`;
  `--mine` and `--review-requests` do not combine: `::mine_and_review_requests_do_not_combine`.
- The screen renders rows, applies events, and opens the pane
  (`pull-requests-page.test.tsx::renders rows, updates from an event, and opens the floating panel`),
  lists every open request by default and narrows to mine or to review requests, adding none by hand
  (`::lists every open request by default, narrows to mine or to review requests, and adds none by hand`),
  narrows by text (`::narrows the rows to the words of their title or description`),
  and holds no button: a click on the row opens the panel.
- The sidebar lists Forge beside the other screens:
  `app-shell.test.tsx::ends the navigation with stats, and lists Forge beside the other screens`.
  Its tabs are routes of their own: `forge-page.test.tsx::shows the tab its URL names, and a tab click moves to the other`.
- The panel reads its facts by name, renders the description, links the forge and each failed check, lists the review sessions, and removes a user row:
  `pull-request-panel.test.tsx::reads its facts by name, renders its description, links the forge, and removes a hand-added row`,
  `::links each failed check to its run, and lists the request's review sessions`.
- Opening a task or session replaces the request panel and preserves list filters:
  `paths.test.ts::pull request panel navigation`.

- A new comment by another login is stored, counted, and told once by id to the author of the task that opened the request; a failed check is told by name; the request has no session of its own:
  `kept_requests.rs::the_news_of_its_request_reaches_the_author_once`.
- The author replies, answering the thread with nothing resolved, and reports; another session and no session get 403:
  `kept_requests.rs::the_author_replies_and_reports_and_no_other_session_may`.
- The task stays approved and its finish is refused until a merge, which is told to the author, whose finish it then accepts; the cleanup is recorded:
  `kept_requests.rs::a_merge_is_told_to_the_author_whose_finish_it_then_accepts`.
- A close is told to the author and finishes nothing:
  `kept_requests.rs::a_close_is_told_to_the_author_and_finishes_nothing`.
- A check that recovers and fails again is told again:
  `kept_requests.rs::a_check_that_recovers_and_fails_again_is_told_again`.
- The detail fetch reads every page of threads and check runs:
  `kept_requests.rs::the_detail_fetch_reads_every_page_of_threads_and_check_runs`.
- A merged goal branch goes once its task is over, and a failed remote delete is tried again by a restarted daemon:
  `kept_requests.rs::a_merged_goal_branch_goes_once_its_task_is_over_and_a_failed_remote_delete_is_tried_again`.
- A closed request the user reviews deletes no branch:
  `kept_requests.rs::a_closed_request_i_review_deletes_no_branch`.
- A request of mine keeps one identity whichever of its task and the fetch comes first, and is not added by hand:
  `pull_requests.rs::a_request_of_mine_keeps_one_identity_whichever_birth_comes_first`.
- A fetch publishes `issues_changed` only when the open issues moved:
  `pull_requests.rs::a_fetch_publishes_issues_changed_only_when_the_open_issues_moved`.
- An approval before the checks end, the checks ending green, and checks going back to pending are told:
  `news.rs::tests::a_change_of_the_checks_is_told_for_the_ready_report`.
- An idle request session is not nudged or relaunched, and a wedged one is relaunched with its briefing:
  `scheduler_attention.rs::an_idle_pull_request_session_is_waiting_on_the_forge_and_a_wedged_one_is_relaunched`.
- The session's MCP server is told the request and no goal:
  `adapters.rs::a_pull_request_session_tells_its_mcp_server_the_request_and_no_goal`.
- The briefing and the news fill every placeholder they name:
  `prompts.rs::tests::the_pull_request_texts_fill_every_placeholder_they_name`.
- Each change is told once, and a green check is news again when it fails again:
  `news.rs::tests::each_change_is_told_once`, `::a_quiet_request_is_no_news`.
- The unanswered count follows the threads, a claim of the same or a stale news is refused, a released claim makes a comment news again, a re-read keeps `told_at`, and a later comment opens a thread again:
  `store.rs::unanswered_comments_count_the_threads_another_login_spoke_last_in`.
- A ready report moves once, and a state is one of three:
  `store.rs::a_pull_request_reports_ready_once_and_takes_its_state`.
- The session migration keeps every old session and request, marks an ended request cleaned, and a backup restores:
  `store.rs::pull_request_session_migration_preserves_sessions_and_requests`.
- The author lists the request's tools beside its task tools:
  `mcp.rs::tests::the_author_lists_the_request_tools_beside_its_task_tools`.
- Its request tools find the request its task opened, then call its routes, and say to open one where there is none:
  `tools.rs::tests::the_authors_request_tools_call_the_routes_of_the_request_its_task_opened`,
  `::an_author_with_no_request_is_told_to_open_one`.
- The migration that gives requests to their authors drops a request of mine no task opened and its session, and keeps task and review rows:
  `store.rs::the_migration_that_gives_requests_to_their_authors_keeps_task_and_review_rows`.
- The skill is within its caps, names no forge CLI, timer, poll or resolve, sends every fix through the reviewers, ends the task on the merge, and ends the turn:
  `defaults.rs::tests::the_pr_babysit_skill_is_fed_by_the_daemon_and_ends_its_turn`, `::skill_size_caps_hold`.
- `session ls` titles a request session with its title and URL:
  `session.rs::tests::a_pull_request_session_shows_the_request_title_and_url`.
- `ariadne attention` lists a request ready to merge by its title:
  `attention.rs::tests::a_session_is_reported_for_the_reason_the_ui_would_give`,
  `board.rs::tests::a_pull_request_ready_to_merge_is_listed_by_its_title`.
- The sessions screen names a request session by its request, linked to the forge, and says it is ready to merge:
  `sessions-page.test.tsx::names a pull request session by its request, linked to the forge, and says it is ready to merge`.
- The session panel shows the request, linked to the forge, in place of a goal and a task:
  `session-panel.test.tsx::shows a pull request session's request, linked to the forge, in place of a goal and a task`.
- The attention list carries `waiting_user` of a request session as ready to merge and opens its session panel:
  `attention.test.tsx::carries a pull request session's waiting_user as ready to merge, opening its session panel`,
  `::keeps a pull request session off the board, having no card and no lane`,
  `attention-strip.test.tsx::lists a pull request ready to merge by its title and opens its session panel`.
- The Pull requests screen row opens its session and shows the unanswered count; the panel links each failed check and the session:
  `pull-requests-page.test.tsx::opens a request's session panel from its row and shows its unanswered comments`,
  `pull-request-panel.test.tsx::links each failed check to its run and the session to its own panel`.
- A session event of a request refetches that request:
  `dispatch.test.ts::refetches the request a session watches when that session starts or moves, for its session_id`,
  `::leaves the pull requests alone for a session that watches none`.
- The session listing takes `pull_request` in OpenAPI:
  `session_list.rs::the_query_and_the_page_are_in_the_openapi_document`.

## Sources

`crates/ariadne-store/migrations/0007_pull_requests.sql`,
`crates/ariadne-store/migrations/0010_pull_request_sessions.sql`,
`crates/ariadne-store/migrations/0012_authors_keep_their_requests.sql`,
`crates/ariadne-store/migrations/0015_pull_request_body.sql`,
`crates/ariadne-store/src/pull_requests.rs`, `crates/ariadne-store/src/pull_request_comments.rs`,
`crates/ariadne-store/skills/pr-babysit/SKILL.md`, `crates/ariadne-api/src/pull_requests.rs`,
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-cli/src/commands/mcp.rs`, `crates/ariadne-cli/src/commands/mcp/tools.rs`,
`crates/ariadne-daemon/src/forge/`, `crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/http/landing.rs`, `crates/ariadne-daemon/src/bus.rs`,
`crates/ariadne-cli/src/commands/pr.rs`, `ui/src/features/pull-requests/`.
