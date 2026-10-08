---
id: pull-requests
status: current
updated: 2026-10-09
areas: [store, api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/src/forge/mod.rs
  - crates/ariadne-daemon/src/forge/news.rs
  - crates/ariadne-daemon/src/forge/live.rs
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

Ariadne reads pull requests live off the forge and stores none of what the forge holds.
It keeps a row only for a request it works on — the request a task opened, one that asks for the user's review on a repository with a review pin, and one of the user's they asked Ariadne to review (029) — and lets go of it once the request merged or closed.
A request a task opened is kept by that task's author until a human merges or closes it (005): the daemon feeds that author the request's news.

## Scope

In: request identity, the rows Ariadne keeps, forge reads, fetch modes, HTTP, events, CLI, and desktop views.
Also in: the author who keeps a request, its comments and checks, the detail read, the news, the tools, and the cleanup.

Out: webhook delivery, and review sessions, on a request that asks for the user's review or one of theirs they asked Ariadne to review ([029](029-reviewing-a-request.md)).
Forge detection and integration settings belong to [025](025-forge-integration.md).
Task completion belongs to [005](005-how-a-task-ends.md).

## Behavior

1. A request has one identity: `(repository_id, number)`.
   The database enforces this pair as unique.
   `PullRequestRef::parse(url, integration)` resolves every request Ariadne starts to work on before `Store::upsert_pull_request` writes it.
   The parser accepts HTTP or HTTPS, trailing slashes, queries, and fragments.
   Host, owner, and repository name comparisons ignore case.
   GitHub uses `/pull/<number>`.
   GitLab accepts `/merge_requests/<number>` and `/-/merge_requests/<number>`, including subgroup paths.
   A different repository or host does not match.
2. Nothing the forge holds is stored: a stored title, check or comment goes stale the moment somebody pushes.
   A `pull_requests` row (`PullRequestRow`) is Ariadne's bookkeeping of a request it works on: `id`, `repository_id`, `number`, `url`, `role`, `origin_task_id`, `ready`, the told marks (rule 18), `reviewed_sha`, `review_asked` and its pin and skills, and `summary_comment_id` (029).
   Migration `0020` dropped every other column, and the comments table.
   `role` is `author` when the request's author is the integration login, ignoring case, else `reviewer`.
   An upsert keeps the existing id and creation time, and the first origin task.
3. A row exists while Ariadne works on the request, and only then:
   - the request a task opened: `open_pull_request` writes it, with the task as origin (005 rule 6);
   - a request that asks for the user's review, out of draft, on a repository whose integration names a `review_model`: the first fetch that finds it writes it (029);
   - a request of the user's they asked Ariadne to review: the ask writes it (029).
   A request nobody works on has no row, whatever the lists hold.
4. What the forge says of a request Ariadne works on is read on every fetch and held in memory alone (`forge::live::LivePulls`, by row id): the request's own read, whether it asks for the user's review, and, while it is open, its details.
   The scheduler and the routes join the row to that read (`PullRequest`, the view). A daemon that restarts holds nothing until its first fetch, which runs at once; until a request has been read, the scheduler does nothing for it.
5. A repository fetch lists every open request of the repository, and the ones that ask for the login's review.
   It starts the work on each review request rule 3 names, then reads each row of the repository: the list's read where the list holds it, else a read of its own — a merged or closed request, or one the list misses.
   A row that asks for review keeps asking while the list of review requests holds it; one it no longer holds, and that asked on the last read, is asked of the forge's timeline (029 rule 3).
6. `ForgePoll::wake(repository_id)` schedules one fetch immediately.
   Each repository has one worker, so fetches of that repository never overlap.
   Wakes during a fetch coalesce into one following fetch.
   `set_mode(repository_id, Mode::Timer)` enables the fallback timer.
   `Timeouts::forge_poll` defaults to 5 minutes.
   Each fetch also reads the repository's open issues, and publishes `issues_changed` with the repository id where they moved since that worker's last read (028).
   `Mode::WakeOnly` has no timer fetches; only explicit wakes fetch.
   Enabled integrations start with one immediate fetch.
   [027](027-webhooks-and-tunnel.md) chooses `WakeOnly` for a live hook and `Timer` otherwise.
   Enabling starts one immediate fetch; disabling cancels its worker and wakes the scheduler for each row of the repository (rule 25).
   Daemon startup fetches every enabled integration once.
7. `unanswered_comments` counts unresolved threads whose last comment is by the other side: a login other than the integration login, or, on a request of the user's own, an Ariadne review (029).
   A thread answered last by the integration login's own side counts zero. A resolved thread counts zero.
   It is worked out from the comments as the forge holds them and the marks (`forge::live`), never stored.
8. A fetch publishes `pull_requests_changed` with the repository id where its requests moved since that worker's last read: the open list, the review requests, and what the requests Ariadne works on read now.
   A row Ariadne starts or stops working on, or one whose ready flag or review ask moves, publishes it too.
   The event carries no request: the desktop reads its lists and panels again (012).

## The author who keeps a request

9. A row with role `author` and an origin task is kept by that task's author (005).
   It gets no session of its own: the author's own session is told its news.
   The author is the picked winner's session on a task with several authors, the lone author's everywhere else.
10. The daemon loads the `pr-babysit` skill for every author of a task that lands by request: a task of a `pull_request` goal, or the final task of a `feature_branch` goal (017).
    It is how the author keeps the request: answer comments, fix checks, keep the branch current, and send every fix through `request_review` before it is pushed.
11. An author whose agent went away while its request is open is picked up by the task's own pass, with the keep-request briefing: push what an approved revision added, read the request, and wait for its news (005 rule 8).
    The same briefing picks up an author approved again after a revision.
    A restart that resumes the author while its open request still reads ready raises `waiting_user` again.
12. The scheduler reconciles requests on every full pass, on every request change, and on every event of a request's session (`scheduler/pull_requests.rs`).
    An idle author on an approved task with an open request waits on the forge: it is never nudged for sitting idle (009 rule 41).
    A disabled integration tells the author nothing; its task stays approved until the integration is on again.

## Comments, checks and the detail read

13. Each fetch reads the details of every open request Ariadne works on.
    `ForgeClient::details(number)` answers the request, every comment with its thread and author, the failed checks, and whether the head is behind the base.
    GitHub reads `gh api` review comments, issue comments and reviews, thread ids and resolution through `gh api graphql`, check runs of the head, and a base compare.
    Every list is read with `--paginate`, the review threads and the check runs included.
    GitLab reads `glab api` discussions, the jobs of the latest pipeline, and the diverged commit count.
    One detail read is bounded by `Timeouts::forge_details`, 30 seconds by default. One that fails keeps the details the last read found.
14. A comment is the forge's, and its id is the forge's: `rc-<n>`, `ic-<n>`, `rv-<n>` or `note-<n>`.
    `kind` is `review_comment`, `issue_comment` or `review`. A review with no body is no comment.
    Comments on the conversation and review bodies share one thread, `conversation`.
    `answered` holds where a later comment in the thread is the integration login's own side's.
    What the database keeps of one is a mark (`pull_request_comment_marks`, migration `0020`): `told_at`, once its session was told of it, and `from_review`, once an Ariadne review posted it (029). The marks go with their row.
    What Ariadne posts itself is held beside the last read until the next fetch reads it back.
15. The DTO joins the forge's read and the row: the request's fields, `failed_checks` and `behind_base`, and, where Ariadne works on it, its `id`, `origin_task_id`, `ready`, review ask, and `session_id` — the newest author session of the origin task for a row a task opened, else the newest review session on the request. A request nobody works on has a null `id`.

## The news

16. `forge/news.rs` computes what the session has not been told: comments of the other side with no `told_at` mark in a thread nobody answered or resolved, checks that turned to failure, a rolled-up check state that changed, `behind_base` that turned true, a review decision that changed, and a state that turned `merged` or `closed`.
    The check state is what a ready report turns on: checks that end green after an approval, or go back to pending after a report, are news.
    The prompt lists each item on one line, and names each comment by its id.
17. The scheduler hands the news as a `session/prompt`, queued behind a running turn (009 rule 6).
    A session still `starting` gets the news once it has its briefing.
    A fetch that finds nothing new hands nothing.
18. Each change is told once (009 rule 4).
    A told comment's mark carries `told_at`. The row keeps a told mark for the rest: the failed checks by name, `behind_base`, the check state, the review decision and the state.
    The ACP driver writes both right before the prompt goes out, as it claims an agent message (018). A prompt that never went out, a kill before it included, gives them back.
    The claim is a compare and set: it holds only while the row still carries the mark the news was computed from, and every comment it names is untold. A stale news is refused, its prompt is skipped, and the next pass computes the news again.
    News of a request still queued is not queued again.
    A check that turns green, or a head that catches up, writes the mark at once with no prompt, so its next turn is news again.

## Tools and replies

19. The author seat lists `get_pull_request`, `list_comments`, `get_comment`, `reply_comment` and `report_pull_request` beside its task tools (013).
    `get_pull_request`, `list_comments` and `get_comment` read the forge at the call: what a session works from is never an earlier read.
    Each finds the request its task opened through `GET /v1/pull-requests?task=<id>&role=author`; with none, it says to call `open_pull_request` first.
    No author tool resolves a thread. A review session resolves a thread it opened, once a push fixed it (029); every other thread is its author's to resolve.
20. A reply posts through the forge CLI: `gh api .../pulls/<n>/comments/<id>/replies` on the thread's first review comment, else `gh pr comment`; GitLab adds a note to the discussion.
    A review session's reply is marked `from_review`. The repository is fetched again, which reads the reply back.
21. A report with `ready: true` on a change raises `waiting_user` on the author's session. `ready: false` on a change clears it. A repeat raises nothing.
    The state is the forge's to say: no session reports it.
    Comments, replies and reports are accepted from the request's own sessions alone: the author of its origin task, or its review session (029). Another session gets 403, and so does a report or reply with no session. The user reads comments freely.
22. `finish_task` of a `pull_request` task is accepted once the forge, read at the call, says its request merged (005 rule 12). A close is told to the author, which fails the task.

## Cleanup

23. A `merged` or `closed` request is told to its author like any other news; the author finishes or fails the task.
    A merged request whose task is still `approved` once the author read the news is finished by the daemon (005 rule 9), which stops the author.
    Once the task is over, what is left of the request is taken down: any session or worktree an earlier release started on it, and, on a merge whose head is a goal branch, that goal branch, local and remote.
    The task's own cleanup took its worktree and its branch.
    Then Ariadne stops working on the request: its row and its marks go, so an ended request takes no room. Its sessions stay, with their history and spend, let go of it.
    A deletion that fails keeps the row: the next change of the request tries it again, and the tick after 60 seconds, and so does a restarted daemon.
    A task that ends while the last read says its request is open reads the forge again first: a finish reads it at the call, so a merge no fetch has read yet still has its work taken down. A request still open lets go of it too, unless the user asked Ariadne to review it.
24. A head branch that is a `goal_repositories.goal_branch` is the goal's.
    A merged request deletes it, local and remote. A closed one deletes neither.
25. With the integration disabled, nothing reads the forge for its requests: each review session ends and its worktree goes, and Ariadne stops working on every request but a task's. Nobody merged or closed anything, so nothing is told and no branch is deleted.
    A task's row stays, and the next fetch with the integration on reads it again.
26. Nothing adds or removes a request by hand, from the desktop or the CLI: Ariadne starts and stops working on requests on its own.

## HTTP

All routes appear in OpenAPI, under the rules of [012](012-http-api-events-and-usage.md).

| Route | Behavior |
| --- | --- |
| `GET /v1/pull-requests?repo=&role=&requested=` | List the open requests of the enabled repositories, or of one, live off the forge: every one, or by role and whether a request asks for the user's review, each joined to its row where Ariadne works on it. An explicit repository with its integration off answers 409. |
| `GET /v1/pull-requests?task=` | The request a task opened, while Ariadne works on it. |
| `GET /v1/pull-requests/{id}` | A request Ariadne works on, read off the forge now. |
| `GET /v1/repositories/{id}/pull-requests/{number}` | One request of a repository, read off the forge now with its checks and comments, joined to its row where there is one. What the desktop's panel reads. |
| `GET /v1/repositories/{id}/pull-requests/search?q=` | Search open requests live by number, title, or author, the user's own left out. Each match says with `tracked` whether Ariadne works on it. |
| `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-review` | Take `{asked}`: ask Ariadne to review a request of the user's own, which starts its work on it, or stop asking (029). |
| `POST /v1/pull-requests/refresh?repo=` | Wake one repository or every enabled repository. Return 202. |
| `GET /v1/pull-requests/{id}/comments?unanswered_only=` | The comments, read off the forge now, with their marks; or the threads that wait on the login. |
| `GET /v1/pull-requests/{id}/comments/{comment_id}` | One comment, read off the forge now. |
| `POST /v1/pull-requests/{id}/comments/{comment_id}/reply` | Post a reply through the forge CLI. Return 201. |
| `POST /v1/pull-requests/{id}/comments/{comment_id}/resolve` | Resolve, from the review session, a thread it opened (029). Return 200. |
| `POST /v1/pull-requests/{id}/report` | Take `ready`, and `reviewed_sha` (029), from the request's own session. |
| `GET /v1/sessions?pull_request=` | List the sessions of one request (012). |

There is no route that adds or removes a request.

## CLI and desktop

The command and screen follow [014](014-command-line-interface.md) and [015](015-desktop-app.md).

- `pr ls [--repo <id|path>] [--mine | --review-requests] [--watch]` lists every open request live, the user's own with `--mine` (`role=author`), or the ones that ask for their review with `--review-requests` (`role=reviewer&requested=true`); the two do not combine. Its author column reads "you" on a request of the user's, and its `ariadne` column says whether Ariadne keeps it for a task, reviews it, or does nothing with it.
- `pr inspect <repo> <number>` prints every field of one request, read off the forge now.
- `pr search --repo <id|path> <query>` searches the forge.
- `pr refresh [--repo <id|path>]` requests a fetch.

The sidebar's Forge entry opens the Forge screen, titled "Forge", whose two tabs are Pull requests (`#/forge/pull-requests`, where `#/forge` lands) and Issues (028).
Pull requests lists the open requests, read live. A three-way choice narrows them: All (every open request), Mine (`role=author`), and Review requests (`role=reviewer&requested=true`).
A text box narrows the rows to the words of their number, title, description, author or head branch, and a repository picker to one enabled repository. Every filter is kept in the URL.
A row shows the linked number and title with a Draft or Ariadne reviewing pill, and under it the repository, "by you" or the author, and the head and base branches; then pills for the checks and the review decision, the unanswered comments, a link to the session, and the update age.
A row holds no button: a click on it, or Enter on it, opens the request's panel.
The header offers Refresh only while a repository in view is not pushed live: the tunnel is off, the tunnel or its hook is down, or the last fetch failed. With every webhook live the events bring each change and there is no Refresh.
A row opens the floating pane through `?pr=<repository>:<number>` and `paths.pullRequest`: a request is named by its repository and number, which every request has, and not by Ariadne's id, which only the requests it works on have.
The panel reads the request off the forge by its repository and number. Its header carries the number and title, pills for the state, the checks, the review decision and an Ariadne review, Ariadne's id where it works on it, and when it opened and moved.
Under it are the facts by name — a link to the request on the forge, the repository, the author, the branches, the head, the unanswered comments, the base, what Ariadne does with it, the task that keeps it and each failed check linked to its run — then two tabs: Description, the body rendered as Markdown, and Sessions, the request's review sessions and the task's author.
Picking a session drills into it in the panel, as `?session=` beside `?pr=`, with a way back to the request.
The panel offers Start review and Stop review on a request of the user's own (029). Nothing on the screen adds or removes a request.
`pull_requests_changed` invalidates every pull request key; no screen polls.
A row and the panel link to the request's session. The panel shows the failed checks and `behind_base`.
A session with a `pull_request_id` shows the request's title and a link to its URL in the sessions table Context column and the session panel.
`ariadne session ls` titles such a session with the request's title and URL, read off the forge. `session inspect` prints its `pull request`.

## Acceptance criteria

- URL variants share one identity and foreign repositories fail:
  `forge/mod.rs::tests::pull_request_urls_share_the_repository_and_number_only`.
- A row keeps its identity and first origin, a role is author or reviewer, and its sessions outlive it:
  `store.rs::a_pull_request_row_keeps_its_identity_and_origin_and_its_sessions_outlive_it`.
- Comment marks are claimed once, released whole, keep the review's mark, and go with their row:
  `store.rs::comment_marks_are_claimed_once_released_whole_and_keep_the_review_mark`.
- The migration that drops the forge's content keeps the rows of work and their marks, drops every other row and column, and lets the sessions of a dropped row go:
  `store.rs::the_migration_that_drops_forge_content_keeps_the_rows_of_work_and_their_marks`.
- The list is read live, a request nobody works on has no row, and nothing adds or removes one:
  `pull_requests.rs::the_list_is_read_live_and_nothing_is_stored_for_a_request_nobody_works_on`.
- A review request on a repository with a review pin has a row while it is reviewed and none once it merged:
  `pull_requests.rs::a_review_request_has_a_row_while_it_is_reviewed_and_none_once_it_merged`.
- A request of mine has a row once a task opened it, and not before:
  `pull_requests.rs::a_request_of_mine_has_a_row_once_a_task_opened_it_and_not_before`.
- A daemon that starts again reads a row the list no longer holds, and finds the merge:
  `pull_requests.rs::startup_fetches_enabled_repositories_and_reads_a_missing_row_as_merged`.
- Disabling during a fetch starts no work:
  `pull_requests.rs::disabling_during_a_fetch_cancels_it_without_starting_work`.
- Timer, WakeOnly, enable, refresh, disable, and disabled refusals work:
  `pull_requests.rs::timer_wake_only_and_disable_control_repository_fetches`.
- Wakes coalesce without concurrent fetches:
  `pull_requests.rs::wakes_during_a_fetch_coalesce_into_one_following_fetch`.
- GitLab reads fork clone URLs, checks, approvals, and reviewer roles; search matches number, author, or title:
  `pull_requests.rs::gitlab_requests_keep_fork_checks_review_and_search_by_author_or_number`.
- A fetch publishes `issues_changed` and `pull_requests_changed` only when they moved:
  `pull_requests.rs::a_fetch_publishes_issues_changed_only_when_the_open_issues_moved`,
  `::a_fetch_publishes_pull_requests_changed_only_when_the_requests_moved`.
- A thread waits on the login until it answers, a comment is told once, a reviewer hears of its own threads, and an Ariadne finding on my request is its author's while the answer is the review's:
  `live.rs::tests::a_thread_waits_on_the_login_until_it_answers_and_a_comment_is_told_once`,
  `::a_reviewer_hears_of_the_replies_in_its_own_threads`,
  `::an_ariadne_finding_on_my_request_is_the_authors_and_its_answer_the_reviews`.
- CLI actions use the live routes and preserve search text, `pr ls` asks every open request, mine, or the review requests, and no command adds or removes one:
  `pr.rs::tests::pr_commands_use_the_live_routes_and_preserve_search_text`;
  `--mine` and `--review-requests` do not combine: `::mine_and_review_requests_do_not_combine`.
- The screen renders rows, reads again on `pull_requests_changed`, and opens the pane by repository and number
  (`pull-requests-page.test.tsx::renders rows, updates from an event, and opens the floating panel`),
  lists every open request by default and narrows to mine or to review requests, adding none by hand
  (`::lists every open request by default, narrows to mine or to review requests, and adds none by hand`),
  narrows by text (`::narrows the rows to the words of their title or description`),
  offers Refresh only while a repository in view polls or its webhook is down
  (`::offers a refresh only while a repository in view polls or its webhook is down`),
  and holds no button: a click on the row opens the panel.
- The sidebar lists Forge beside the other screens:
  `app-shell.test.tsx::ends the navigation with stats, and lists Forge beside the other screens`.
  Its tabs are routes of their own: `forge-page.test.tsx::shows the tab its URL names, and a tab click moves to the other`.
- The panel reads the request by its repository and number, its facts by name, renders the description, links the forge and each failed check, lists the review sessions, and removes nothing:
  `pull-request-panel.test.tsx::reads its facts by name, renders its description, links the forge, and removes nothing`,
  `::links each failed check to its run, and lists the request's review sessions`.
- Opening a task or session replaces the request panel and preserves list filters:
  `paths.test.ts::pull request panel navigation`.
- A new comment by another login is told once by id to the author of the task that opened the request; a failed check is told by name; the request has no session of its own:
  `kept_requests.rs::the_news_of_its_request_reaches_the_author_once`.
- The author replies, which the next read finds answering the thread, and reports; its resolve is refused; another session and no session get 403:
  `kept_requests.rs::the_author_replies_and_reports_and_no_other_session_may`.
- The task stays approved and its finish is refused until a merge, which is told to the author; the daemon then finishes the task, the author is stopped, and the row goes:
  `kept_requests.rs::a_merge_is_told_to_the_author_and_then_ends_the_task_and_its_agent`.
- A close is told to the author and finishes nothing:
  `kept_requests.rs::a_close_is_told_to_the_author_and_finishes_nothing`.
- A check that recovers and fails again is told again:
  `kept_requests.rs::a_check_that_recovers_and_fails_again_is_told_again`.
- The detail read reads every page of threads and check runs:
  `kept_requests.rs::the_detail_fetch_reads_every_page_of_threads_and_check_runs`.
- A merged goal branch goes once its task is over, and a failed remote delete is tried again by a restarted daemon:
  `kept_requests.rs::a_merged_goal_branch_goes_once_its_task_is_over_and_a_failed_remote_delete_is_tried_again`.
- A closed request the user reviews deletes no branch, and its row goes:
  `kept_requests.rs::a_closed_request_i_review_deletes_no_branch`.
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
- A ready report moves once:
  `store.rs::a_pull_request_reports_ready_once`.
- The session migration keeps every old session:
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
- The Pull requests screen row opens its session and shows the unanswered count:
  `pull-requests-page.test.tsx::opens a request's session panel from its row and shows its unanswered comments`.
- A session event of a request refetches that request:
  `dispatch.test.ts::refetches the request a session watches when that session starts or moves, for its session_id`,
  `::leaves the pull requests alone for a session that watches none`.
- `pull_requests_changed` reads every request key again:
  `dispatch.test.ts::reads every request again on pull_requests_changed, which carries none`.
- The session listing takes `pull_request` in OpenAPI:
  `session_list.rs::the_query_and_the_page_are_in_the_openapi_document`.

## Sources

`crates/ariadne-store/migrations/0007_pull_requests.sql`,
`crates/ariadne-store/migrations/0010_pull_request_sessions.sql`,
`crates/ariadne-store/migrations/0012_authors_keep_their_requests.sql`,
`crates/ariadne-store/migrations/0020_pull_requests_hold_no_forge_content.sql`,
`crates/ariadne-store/src/pull_requests.rs`, `crates/ariadne-store/src/pull_request_comments.rs`,
`crates/ariadne-store/skills/pr-babysit/SKILL.md`, `crates/ariadne-api/src/pull_requests.rs`,
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-cli/src/commands/mcp.rs`, `crates/ariadne-cli/src/commands/mcp/tools.rs`,
`crates/ariadne-daemon/src/forge/`, `crates/ariadne-daemon/src/forge/live.rs`,
`crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/http/landing.rs`, `crates/ariadne-daemon/src/bus.rs`,
`crates/ariadne-cli/src/commands/pr.rs`, `ui/src/features/pull-requests/`.
