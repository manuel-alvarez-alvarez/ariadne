---
id: pull-requests
status: current
updated: 2026-10-14
areas: [store, api, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/src/forge/mod.rs
  - crates/ariadne-daemon/src/forge/news.rs
  - crates/ariadne-daemon/src/forge/live.rs
  - crates/ariadne-daemon/src/forge/pulls.rs
  - crates/ariadne-daemon/src/forge/github/pulls.rs
  - crates/ariadne-daemon/src/forge/github/comments.rs
  - crates/ariadne-daemon/src/forge/github/reviews.rs
  - crates/ariadne-daemon/src/forge/github/details.rs
  - crates/ariadne-daemon/src/forge/gitlab/comments.rs
  - crates/ariadne-daemon/src/forge/gitlab/reviews.rs
  - crates/ariadne-daemon/src/forge/gitlab/details.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/tests/it/pull_requests.rs
  - crates/ariadne-daemon/tests/it/pull_request_detail_skip.rs
  - crates/ariadne-daemon/tests/it/pull_request_lists.rs
  - crates/ariadne-daemon/tests/it/forge_rate_limit.rs
  - crates/ariadne-daemon/tests/it/tunnel.rs
  - crates/ariadne-daemon/tests/it/kept_requests.rs
  - crates/ariadne-daemon/tests/it/pull_request_keep_retry.rs
  - crates/ariadne-daemon/tests/it/workflow_pull_request.rs
  - crates/ariadne-daemon/tests/it/scheduler_attention.rs
  - crates/ariadne-daemon/tests/it/adapters.rs
  - crates/ariadne-daemon/tests/it/session_list.rs
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
  - ui/src/events/dispatch.test.ts
  - ui/src/api/queries.test.ts
---

# Pull requests

Ariadne reads pull requests off the forge and stores none of what the forge holds, in memory alone.
A repository's open list is held from its last fetch; a request Ariadne works on keeps its own read and details the same way.
Nothing here keeps either in SQLite, and every one of them is empty until the first fetch lands.
It keeps a row only for a request it works on — the request a task opened, one that asks for the user's review, and one of the user's they asked Ariadne to review (029) — and lets go of it once the request merged or closed.
The agent of the task's `pr` column keeps its request until a human merges or closes it (030). The daemon sends news to that session.

## Scope

In: request identity, the rows Ariadne keeps, forge reads, fetch modes, HTTP, events, CLI, and desktop views.
Also in: the agent who keeps a request, its comments and checks, the detail read, the news, the tools, and the cleanup.

Out: webhook delivery, and review sessions, on a request that asks for the user's review or one of theirs they asked Ariadne to review ([029](029-reviewing-a-request.md)).
Also out: the keeper session on a request of the user's own asked for by hand, and its worktree and branch ([032](032-babysitting-a-request-by-hand.md)).
Forge detection and integration settings belong to [025](025-forge-integration.md).
The `pr` column, its gate and how its task ends belong to [030](030-workflows.md).

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
   A `pull_requests` row (`PullRequestRow`) is Ariadne's bookkeeping of a request it works on: `id`, `repository_id`, `number`, `url`, `role`, `origin_task_id`, `ready`, `ready_head_sha` (the head the `ready` claim was confirmed on, 031 rule 13), the told marks (rule 18), `reviewed_sha`, `review_asked` and its pin and skills, `babysit_asked` and its pin and skills (032), and `summary_comment_id` (029). The row holds no other column, and there is no comments table.
   `role` is `author` when the request's author is the integration login, ignoring case, else `reviewer`.
   An upsert keeps the existing id and creation time, and the first origin task.
3. A row exists while Ariadne works on the request, or while a human still
   might start it working by hand, and only then:
   - the request a task opened: `open_pull_request` writes it, with the task as origin (030 rule 10);
   - a request that asks for the user's review, out of draft: the first fetch that finds it writes it — nothing starts a session on it on its own, but the row is what lets a human start one by hand (029, 031);
   - a request of the user's they asked Ariadne to review: the ask writes it (029);
   - a request of the user's own, opened by no task, they asked Ariadne to keep by hand: the ask writes it (032).
   A request nobody works on, and nobody could start work on by asking, has no row, whatever the lists hold.
4. What the forge says of a request Ariadne works on is read on every fetch and held in memory alone (`forge::live::LivePulls`, by row id): the request's own read, whether it asks for the user's review, and, while it is open, its details.
   The scheduler and the routes join the row to that read (`PullRequest`, the view). A daemon that restarts holds nothing until its first fetch, which runs at once; until a request has been read, the scheduler does nothing for it.
   Each fetch also holds its repository's whole list the same way (`forge::live::LivePulls`, by repository id): every open request and which of them ask for the user's review, as the fetch itself read them, when, and the forge identity that supplied them.
   Disabling an integration or changing that identity clears the held list; a route also rejects a list whose identity differs from its current integration.
   `GET /v1/pull-requests` and its search answer from that held list alone, joined to the rows as the routes above do; neither calls the forge. A repository with no fetch yet holds no list, which both read as empty rather than live.
   `GET /v1/pull-requests/{id}` answers the held read of the row the same way; `GET /v1/repositories/{id}/pull-requests/{number}` stays live, since that is the panel reading one request on demand.
   `POST /v1/pull-requests/refresh` is still how a human asks the forge to be read again.
5. A repository fetch lists every open request of the repository in one read; GitHub names each request's own requested reviewers on that same read (`reviewRequests`), so the login's own review requests need no second read. GitLab still reads its own list and its review requests apart.
   It starts the work on each review request rule 3 names, then reads each row of the repository: the list's read where the list holds it, else a read of its own — a merged or closed request, or one the list misses.
   A row that asks for review keeps asking while the list names the login among its requested reviewers; one it no longer does, and that asked on the last read, is asked of the forge's timeline (029 rule 3).
   GitHub names a team's own request by a slug or a name, never a login, so a review asked of a team the login belongs to is read as asking nobody (Known gap).
6. `ForgePoll::wake(repository_id)` schedules one fetch.
   Each repository has one worker, so fetches of that repository never overlap.
   Wakes during a fetch coalesce into one following fetch.
   A wake outside a fetch starts one `Timeouts::forge_wake_debounce` window before it fetches, folding every further wake that arrives before that fixed deadline into the same one fetch: a burst of webhook deliveries for one push costs one fetch, not one per delivery, even if the deliveries continue through the window.
   `set_mode(repository_id, Mode::Timer)` enables the fallback timer.
   `Timeouts::forge_poll` defaults to 5 minutes.
   A fetch that fails for a reason other than a rate limit, in `Mode::Timer`, doubles the wait before the next timer fetch, up to `Timeouts::forge_poll_backoff_cap` (1 hour by default); a fetch that works resets it to `Timeouts::forge_poll`.
   A wake still fetches at once (after the debounce above), whatever the current wait.
   A fetch that answers a forge rate limit pauses the worker in either mode until the latest reset time of an exhausted forge resource, read with no quota of its own (`ForgeClient::rate_limit_reset`), or `Timeouts::forge_rate_limit_pause` (15 minutes by default) from now where that read fails or no primary resource is exhausted; the pause lasts at least `Timeouts::forge_poll`, so a reset already past cannot spin the worker. Every wake inside the pause folds into the one fetch that runs once it ends. `fetch_error` reads `rate limited until <RFC 3339 time>` while paused (`forge::poll::FORGE_RATE_LIMITED`), and null once a fetch works.
   Each fetch also reads the repository's open issues, and publishes `issues_changed` with the repository id where they moved since that worker's last read (028).
   `Mode::WakeOnly` has no timer fetches; only explicit wakes fetch.
   Enabled integrations start with one immediate fetch.
   [027](027-webhooks-and-tunnel.md) chooses `WakeOnly` for a live hook and `Timer` otherwise.
   Enabling starts one immediate fetch; disabling cancels its worker and wakes the scheduler for each row of the repository (rule 24).
   Daemon startup fetches every enabled integration once.
7. `unanswered_comments` counts unresolved threads whose last comment is by the other side: a login other than the integration login, or, on a request of the user's own, an Ariadne review (029).
   A thread answered last by the integration login's own side counts zero. A resolved thread counts zero.
   It is worked out from the comments as the forge holds them and the marks (`forge::live`), never stored.
8. A fetch publishes `pull_requests_changed` with the repository id where its requests moved since that worker's last read: the open list, the review requests, and what the requests Ariadne works on read now.
   A row Ariadne starts or stops working on, or one whose ready flag or review ask moves, publishes it too.
   The event carries no request: the desktop reads its lists and panels again (012).

## The agent who keeps a request

The current task-column agent owns an author request, its skill, its fixes,
and its idle handling under [030](030-workflows.md), rule 10. It never
checks the request on its own: Ariadne wakes the session with the
request's news instead, a rule the session rules of the `agent` and
`reviewer` seats state once, for every seat that keeps or reviews a
request ([006](006-prompts-and-simplified-technical-english.md) rule 5).

## Comments, checks and the detail read

9. Each fetch reads the details of every open request Ariadne works on, except where the held details are still current under the skip rule below.
    `ForgeClient::details(number)` answers the request, every comment with its thread and author, the failed checks, and whether the head is behind the base.
    GitHub reads `gh api` review comments, issue comments and reviews, thread ids and resolution through `gh api graphql`, check runs of the head, and a base compare.
    Every list is read with `--paginate`, the review threads and the check runs included.
    GitLab reads `glab api` discussions, the jobs of the latest pipeline, and the diverged commit count.
    The reads go to the forge together; only GitHub's check runs and compare wait, on the request's own read, for its head and base.
    One detail read is bounded by `Timeouts::forge_details`, 30 seconds by default. One that fails keeps the details the last read found.
    A fetch skips the detail read of a request whose list read moves nothing a detail read would also show: the held read has details and `evidence_ok`, its `updated_at`, `head_sha`, `checks`, `review_decision`, `mergeable`, `draft`, `title` and `state` all equal the list's own read, its `LivePulls::generation` still equals the read's snapshot, no comment Ariadne posted is still unconfirmed by a detail read, and the details are less than 15 minutes old (`forge::live::Live::unmoved`). The held details stand in place of a fresh read.
    A skipped read also leaves `behind_base` as its held detail value, and does not see a changed set of failed check names where normalized `checks` remains `failure`; the corresponding rule 12 news waits for the next detail read. At 15 minutes, the next fetch reads the details even where every list field still matches. The five-minute fallback timer permits at most two unchanged skips before that refresh.
    A thread a human resolves on the forge does not move `updated_at`; its resolved state is read by that forced refresh when no earlier request move reads it.
    The detail read drops the viewer's own pending review from the conversation outright, its body, its inline comments and its replies alike, and never analyzes, tells or wakes a session for any of them: GitHub's `pulls/{number}/comments` is undocumented for whether it also answers a pending review's own inline comments and replies, and it does — the shape a pull request on this very repository once showed waking a reviewer and a keeping agent, both — so each is dropped by its own `pull_request_review_id` against the ids of every review `pulls/{number}/reviews` already lists with no `submitted_at`, or a `state` still `PENDING` (`forge/github/details.rs::pending_review_ids`). A review thread left with no comment once its only ones were pending has nothing in the list to carry it, so it never surfaces either. GitLab's own draft notes never enter `discussions` at all until the viewer publishes them, so GitLab's detail read needs no filter of its own to keep them out. Either way, a comment Ariadne itself posted into the pending review or the draft notes (rule 24) is tracked apart from this filter, in memory alone, and is never fed through it either (rule 10). A webhook delivery (027) wakes no agent over either kind of pending content on its own: it only calls `ForgePoll::wake`, which runs this same detail read, so the filter above is the whole of its proof too. The detail read also answers whether the viewer's own pending set is still open at all (`ForgeDetails::has_pending_posts`): on GitHub, whether `pulls/{number}/reviews` still lists one with no `submitted_at`, or a `state` still `PENDING`; on GitLab, a count-only `GET .../draft_notes` read, never fed through the filter above, answers whether there is still at least one. While it holds, `merge_fresh_details` carries a review's own finding posted into that pending set past every poll before the viewer publishes, not only the one poll of grace any other local addition gets (rule 25).
10. A comment is the forge's, and its id is the forge's: `rc-<n>`, `ic-<n>`, `rv-<n>` or `note-<n>`.
    `kind` is `review_comment`, `issue_comment` or `review`. A review with no body is no comment.
    Comments on the conversation and review bodies share one thread, `conversation`.
    `answered` holds where a later comment in the thread is the integration login's own side's.
    What the database keeps of one is a mark (`pull_request_comment_marks`): `told_at`, once its session was told of it, and `from_review`, once an Ariadne review posted it (029). The marks go with their row.
    What Ariadne posts itself is held beside the last read until the next fetch reads it back.
11. The DTO joins the forge's read and the row.
    It includes the request's fields, `failed_checks`, `behind_base`, `id`, `origin_task_id`, `ready`, review ask, and `session_id`.
    For an origin task, `session_id` names the current column agent's newest session.
    Otherwise it names the newest review session on the request.
    A request nobody works on has a null `id`.

## The news

12. `forge/news.rs` computes what the session has not been told: comments of the other side with no `told_at` mark in a thread nobody answered or resolved, checks that turned to failure, a rolled-up check state that changed, `behind_base` that turned true, a review decision that changed, and a state that turned `merged` or `closed`.
    The check state is what a ready report turns on: checks that end green after an approval, or go back to pending after a report, are news.
    The prompt lists each item on one line, and names each comment by its id.
13. The scheduler hands the news as a `session/prompt`, queued behind a running turn (009 rule 6).
    A session still `starting` gets the news once it has its briefing.
    A fetch that finds nothing new hands nothing.
14. Each change is told once (009 rule 4).
    A told comment's mark carries `told_at`. The row keeps a told mark for the rest: the failed checks by name, `behind_base`, the check state, the review decision and the state.
    The ACP driver writes both right before the prompt goes out, as it claims an agent message (018). A prompt that never went out, a kill before it included, gives them back.
    The claim is a compare and set: it holds only while the row still carries the mark the news was computed from, and every comment it names is untold. A stale news is refused, its prompt is skipped, and the next pass computes the news again.
    News of a request still queued is not queued again.
    A check that turns green, or a head that catches up, writes the mark at once with no prompt, so its next turn is news again.

## Tools and replies

15. The agent seat lists `get_pull_request`, `list_comments`, `get_comment`, `reply_comment` and `report_pull_request` beside its task tools (013).
    `get_pull_request`, `list_comments` and `get_comment` read the forge at the call: what a session works from is never an earlier read.
    Each finds the request its task opened through `GET /v1/pull-requests?task=<id>&role=author`; with none, it says to call `open_pull_request` first.
    No agent tool resolves a thread. A review session resolves a thread it opened, once a push fixed it (029); every other thread is the keeping agent's to resolve.
16. A reply posts through the forge CLI: `gh api .../pulls/<n>/comments/<id>/replies` on the thread's first review comment, else `gh pr comment`; GitLab adds a note to the discussion.
    A review session's reply is marked `from_review`. The repository is fetched again, which reads the reply back.
    The current column agent's `reply_comment` answers an inline review comment, the thread of the defect. In the conversation, it answers a comment of the other side once. It may not answer a comment of its own side. A `from_review` comment, a review's own summary, is one of those. It may not answer a comment that its own side already answered in the conversation. The daemon reads the seat (`Seat::Agent`), as for rule 17. A review session is never this seat. The daemon refuses any other conversation reply of the agent with 400, and makes no forge call. A review session keeps its wider reach, into the conversation and a defect's thread alike (029).
    No body the daemon posts on a request's behalf — this reply, `open_pull_request`'s own body, and a review's summary and findings (029) — carries a trailer an agent's skill appends: a `Generated with` footer, a `Co-Authored-By`, or a tool's `Signed-off-by`, and the blank line before it, are stripped before the post.
17. A report with `ready: true` or `ready: false` is stored and raises nothing of its own on any session (031): the `pull_request` attention producer reads the forge's own evidence against the claim instead of trusting it. A `ready: true` report also names `head_sha`, the head it was confirmed on (029 rule 15); it is stored as `ready_head_sha` and is rejected before storage when it is missing or fails the same hex-sha check `reviewed_sha` does. A request a task keeps accepts a `ready` report from the current column agent alone, never from a review session that also answers for the same row (a request of mine a task keeps can also carry an Ariadne self-review asked on it, 029) — read by seat (`Seat::Agent`), not merely by which session answers for the row.
    The state is the forge's to say: no session reports it.
    Accept comments, replies, and reports only from the current column agent or the review session (029), subject to the `ready`-specific restriction above.
    Another session gets 403. A report or reply with no session gets 403. The user reads comments freely.
18. `complete_step` on the `pr` column is accepted once the forge, read at the call, says its request merged (030 rule 5). A close is told to the agent, which fails the task.

## Cleanup

19. A `merged` or `closed` request is told to the keeping agent like any other news; the agent completes its step on a merge or fails the task on a close.
    The daemon advances a merged request column itself when that agent falls quiet after the news, and finishes the task when that column is last (030 rule 10).
    Once the task is over, what is left of the request is taken down: any session or worktree an earlier release started on it.
    The task's own cleanup took its worktree and its branch.
    Then Ariadne stops working on the request: its row and its marks go, so an ended request takes no room. Its sessions stay, with their history and spend, let go of it.
    The row goes only as the pass read it: a pass decides who wants a request from a read taken a few store calls earlier, and an ask by hand that lands in between — a review (029), a keeper (032) — is one that read never saw. Deleted regardless, the row would take the ask with it, and the next fetch would read the request back with nothing asked of it; so the delete is refused where an ask was written since the read, the row stays whole with its sessions on it, and the pass the ask itself woke acts on it. Every ask or withdrawal of a review or a keeper, and a task adopting the request, moves the row's ask revision, and the delete holds to that revision: an ask taken back and made again, or made again on another pin, reads as the same asks but is still one the read never saw.
    A deletion that fails keeps the row: the next change of the request tries it again, and the tick after 60 seconds, and so does a restarted daemon.
    A task that ends while the last read says its request is open reads the forge again first: a step completion reads it at the call, so a merge no fetch has read yet still has its work taken down. A request still open lets go of it too, unless the user asked Ariadne to review it.
    An author request with no live task to keep it — one opened by no task, or whose task ended — and still asked for by hand reads the forge for its end on the next change of the request at once, and otherwise no more than once every 60 seconds: a read that finds it still open costs the same wait a failed one does, rather than running on every scheduler tick:
    `pull_request_keep_retry.rs::an_open_kept_request_is_read_once_per_wait`.
20. With the integration disabled, nothing reads the forge for its requests: each review session ends and its worktree goes, and Ariadne stops working on every request but a task's. Nobody merged or closed anything, so nothing is told.
    A task's row stays, and the next fetch with the integration on reads it again.
21. Nothing adds or removes a request by hand, from the desktop or the CLI: Ariadne starts and stops working on requests on its own.
22. `open_pull_request`'s body fills the repository's own pull request template where the repository has one. Where it has none, the body carries three sections, Why, What changed and How to test, each written from the task and its diff rather than one line. The title still follows the repository's commit conventions (030 rule 10).
23. A repository that supervises its posts ([002](002-repositories-branches-and-worktrees.md) rule 13) opens every pull request a task's `pr` column requests as a draft, whatever the agent's own `draft` asked: a person reviews it before it is real to anyone else. A repository that does not leaves the agent's own value through unchanged.
24. A repository that supervises its posts holds every reply this way too: `POST .../reply` calls `ForgeClient::reply_pending` instead of `ForgeClient::reply`, posting nothing to the forge directly, and answers `{pending: true, id}` in place of a comment, `id` the pending item's own forge id. A review holds its inline findings this way. A reviewer leaves thread resolution to the user.
    GitHub finds or creates the viewer's one pending review per request (`Github::pending_review`; GitHub allows only one, so it is reused). One it creates carries the hidden verdict mark, `<!-- ariadne:pending-review-verdict -->`, as its whole body: GitHub refuses to edit a review it holds with no body at all, through either API, and every later change of the pending review is an edit of its body. One it finds with no body — an earlier Ariadne's, or one begun by hand on the forge — is deleted and made again with the mark where it holds no draft comment, and is otherwise the user's to submit or discard first: the post is refused saying so, and nothing of theirs is deleted. A reply to an inline review comment joins the pending review's own thread, `addPullRequestReviewThreadReply`, answering the new draft comment's id. A reply to a top-level conversation comment has no pending form of its own, so the pending review's body gains it instead, led by a quote of the comment it answers and a link to it (`Github::append_to_pending_review`), answering the pending review's own id.
    GitLab posts a draft note instead, `POST .../draft_notes`, `in_reply_to_discussion_id` naming the comment's own discussion; only its own author sees it until it is published.
    The comment this answers is marked `pending_reply` (`pull_request_comment_marks`), a one-way local record never read back off the forge: it counts answered from here on (`forge::live::answered`), so it does not wake the keeping agent again, whatever becomes of the forge's own pending item — a draft deleted there before the user publishes it leaves the mark, and the comment, as they are. Since the mark adds no later comment of its own for `answered` to find, it stands at its own comment's place in the thread instead: every earlier comment of that thread reads answered too, the same as a real reply there would show, and a later one stays open. A repository that does not supervise its posts posts the reply as before, under rule 16.
    The mark lives in the store alone, on no field a forge fetch ever reads back, so the route announces the request changed (`events.pull_requests_changed`) and wakes the scheduler itself: nothing else could tell a watcher the comment now reads answered.
    The agent's own `reply_comment` tool reads a `pending` answer and says, in its own words, that the reply waits in the user's forge drafts and that the agent goes on, rather than handing back the pending shape as though it were the posted comment rule 15 otherwise answers with.
25. Whichever session posted a pending item under rule 24, every session of the request may edit or delete it, since every pending post goes under the one integration login: `POST /v1/pull-requests/{id}/comments/edit` (`{forge_id, body}`), `POST /v1/pull-requests/{id}/comments/delete` (`{forge_id}`), and, GitHub only, `POST /v1/pull-requests/{id}/pending-review/body` (`{body}`), which replaces the pending review's own body outright. The user asking for a change in the console is what calls these, never a round of `reply_comment`, `submit_review` or `resolve_thread` on its own.
    Each is refused with 409 once its target is no longer pending — the user already submitted the review it was in, published the draft note, or there never was one:
    - GitHub: `edit_pending_comment` and `delete_pending_comment` read the viewer's still-pending review (rule 24's own finder, split from its create-if-missing form so a caller that means only to look never brings one into existence), then look up the comment's GraphQL node id among that review's own comments — `updatePullRequestReviewComment` and `deletePullRequestReviewComment` take a node id, never the database id `addPullRequestReviewThread` answers. A comment the lookup does not find, because its round was since submitted, answers 409 the same as no pending review at all. `edit_pending_review_body` overwrites the pending review's body through the same `updatePullRequestReview` mutation rule 24 already uses, refused with 409 where there is no pending review to find.
    - GitLab: `edit_pending_comment` and `delete_pending_comment` write straight to the draft note by its own id, `PUT` and `DELETE .../draft_notes/<id>`; a note GitLab has already consumed into a published note on the user's publish answers 404, read as the 409 above. GitLab holds no pending review body of its own, so `edit_pending_review_body` answers 400 on it.
    The three tools are `edit_pending_comment`, `delete_pending_comment` and `edit_pending_review_body` (013), on the agent, pull request reviewer and pull request keeper seats alike.
    A successful edit or delete also writes Ariadne's own held comments at once (`live::LivePulls::edit_comment`, `::remove_comment`): rule 9 already drops GitHub's pending review from every detail read of its own, and GitLab's own draft notes never enter one at all, so no detail fetch ever reads either change back on its own while the post stays unpublished. Without this, `list_comments` would answer the body a finding was posted with forever, and a deleted finding would stay in `merge_fresh_details`'s own carry-forward for as long as it remained unconfirmed by a fresh read — read by `submit_review`'s own `open_p0` check as a P0 still open.

## HTTP

All routes appear in OpenAPI, under the rules of [012](012-http-api-events-and-usage.md).

| Route | Behavior |
| --- | --- |
| `GET /v1/pull-requests?repo=&role=&requested=` | List the open requests of the enabled repositories, or of one, from each repository's last fetch: every one, or by role and whether a request asks for the user's review, each joined to its row where Ariadne works on it. A repository with no fetch yet answers none of its requests. An explicit repository with its integration off answers 409. |
| `GET /v1/pull-requests?task=` | The request a task opened, while Ariadne works on it. |
| `GET /v1/pull-requests/{id}` | A request Ariadne works on, as the last fetch found it, or read off the forge now where nothing has read it yet. |
| `GET /v1/repositories/{id}/pull-requests/{number}` | One request of a repository, read off the forge now with its checks and comments, joined to its row where there is one. What the desktop's panel reads. |
| `GET /v1/repositories/{id}/pull-requests/search?q=` | Search the repository's last fetched list by number, title, or author, the user's own left out. Each match says with `tracked` whether Ariadne works on it. A repository with no fetch yet matches nothing. |
| `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-review` | Take `{asked}`: ask Ariadne to review a request of the user's own, which starts its work on it, or stop asking (029). |
| `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-babysit` | Take `{asked}`: ask Ariadne to keep a request of the user's own, opened by no task, by hand, which starts its work on it, or stop asking (032). |
| `POST /v1/pull-requests/refresh?repo=` | Wake one repository or every enabled repository. Return 202. |
| `GET /v1/pull-requests/{id}/comments?unanswered_only=` | The comments, read off the forge now, with their marks; or the threads that wait on the login. |
| `GET /v1/pull-requests/{id}/comments/{comment_id}` | One comment, read off the forge now. |
| `POST /v1/pull-requests/{id}/comments/{comment_id}/reply` | Post a reply through the forge CLI. Return 201. A repository that supervises its posts holds it in the user's own forge drafts instead, answering `{pending: true, id}` (rule 24). |
| `POST /v1/pull-requests/{id}/comments/{comment_id}/resolve` | Resolve, from the review session, a thread it opened (029). Return 200. In a supervised repository, leave it for the user. |
| `POST /v1/pull-requests/{id}/comments/edit` | Take `{forge_id, body}`: edit one of the viewer's own pending posts. Return `{id, changed}`. 409 once it is no longer pending (rule 25). |
| `POST /v1/pull-requests/{id}/comments/delete` | Take `{forge_id}`: delete one of the viewer's own pending posts. Return `{id, changed}`. 409 once it is no longer pending (rule 25). |
| `POST /v1/pull-requests/{id}/pending-review/body` | Take `{body}`: replace the viewer's pending review body outright. GitHub only; 400 on GitLab. 409 with no pending review (rule 25). |
| `POST /v1/pull-requests/{id}/report` | Take `ready`, `head_sha` (required with `ready: true`), and `reviewed_sha` (029), from the request's own session. |
| `GET /v1/sessions?pull_request=` | List the sessions of one request (012). |

There is no route that adds or removes a request.

## CLI and desktop

The command and screen follow [014](014-command-line-interface.md) and [015](015-desktop-app.md).

- `pr ls [--repo <id|path>] [--mine | --review-requests] [--watch]` lists every open request from the repository's last fetch, the user's own with `--mine` (`role=author`), or the ones that ask for their review with `--review-requests` (`role=reviewer&requested=true`); the two do not combine. Its author column reads "you" on a request of the user's, its `ariadne` column says whether Ariadne keeps it for a task, reviews it, babysits it by hand (032), both of the last two together, or nothing, its `branches` column reads `head → base`, and its `session` column links the request's newest session.
- `pr inspect <repo> <number>` prints every field of one request, read off the forge now, as a hand-written key-value block in a fixed reading order, the body last after a `---` line (014).
- `pr search --repo <id|path> <query>` searches the repository's last fetched list.
- `pr refresh [--repo <id|path>]` requests a fetch.
- `pr review <repo> <number> --model AGENT:MODEL [--effort E] [--skill S]... [--attach]` calls `PUT .../ariadne-review` with `{asked: true, model, effort, skills}` (029): `--model` is required, since there is no repository pin to fall back to any more, and a usage error names it when it is missing. `--attach` waits for the review session to start and opens its console, as `session new --attach` does. `pr review <repo> <number> --stop` sends `{asked: false}` alone, and conflicts with `--model`, `--effort`, `--skill` and `--attach`.
- `pr babysit <repo> <number> --model AGENT:MODEL [--effort E] [--skill S]... [--attach]` and `pr babysit <repo> <number> --stop` call `PUT .../ariadne-babysit` the same way `pr review` calls its own route (032).

The sidebar's Forge entry opens the Forge screen, titled "Forge", whose two tabs are Pull requests (`#/forge/pull-requests`, where `#/forge` lands) and Issues (028).
Pull requests lists the open requests, from the repository's last fetch. A three-way choice narrows them: All (every open request), Mine (`role=author`), and Review requests (`role=reviewer&requested=true`).
A text box narrows the rows to the words of their number, title, description, author or head branch, and a repository picker to one enabled repository. Every filter is kept in the URL.
A row shows the linked number and title with a Draft, Ariadne reviewing or Ariadne babysitting pill, and under it the repository, "by you" or the author, and the head and base branches; then pills for the checks and the review decision, the unanswered comments, a link to the session, and the update age.
A row holds no button: a click on it, or Enter on it, opens the request's panel.
The header offers Refresh only while a repository in view is not pushed live: the tunnel is off, the tunnel or its hook is down, or the last fetch failed. With every webhook live the events bring each change and there is no Refresh.
A row opens the floating pane through `?pr=<repository>:<number>` and `paths.pullRequest`: a request is named by its repository and number, which every request has, and not by Ariadne's id, which only the requests it works on have.
The panel reads the request off the forge by its repository and number. Its header carries the number and title, pills for the state, the checks, the review decision, an Ariadne review and Ariadne babysitting, Ariadne's id where it works on it, and when it opened and moved.
Under it are the facts by name — a link to the request on the forge, the repository, the author, the branches, the head, the unanswered comments, the base, what Ariadne does with it, the task that keeps it and each failed check linked to its run — then two tabs: Description, the body rendered as Markdown, and Sessions, the request's review and keeper sessions and the task's keeping agent.
Picking a session drills into it in the panel, as `?session=` beside `?pr=`, with a way back to the request.
The panel offers Start review and Stop review on a request of the user's own or one that asks for their review (029), and, on a request of the user's own that no task keeps, Start babysit and Stop babysit (032). Nothing on the screen adds or removes a request.
`pull_requests_changed` invalidates every pull request key; no screen polls.
A `502` whose body names `forge_error` is not retried: the forge itself refused or rate-limited the daemon's own call, which a quick retry would only repeat, and a request a fetch rate-limits is still answered from its last fetch.
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
- A row is deleted only as the pass read it: an ask that landed since the read, a repeated one included, keeps the row and its sessions, and the row goes once read as it is:
  `store.rs::deleting_a_request_is_refused_where_an_ask_landed_since_the_read`.
- The held list answers the fetch's own read, a request of the user's own nobody asked Ariadne to review has no row, a request that asks for the user's review is tracked the moment it is seen, and nothing adds or removes a row by hand:
  `pull_requests.rs::the_fetch_holds_the_list_and_tracks_only_a_request_that_asks_for_my_review`.
- GitHub's repository fetch lists the open requests in one `pr list` call, a user named among a request's own `reviewRequests` asks for that user's review, a team named there asks for nobody's, and a read from before the field existed asks for nobody's:
  `forge/github/pulls.rs::tests::a_user_review_request_names_its_login`,
  `::a_team_review_request_matches_no_login`,
  `::a_read_naming_no_review_requests_asks_for_nobody`;
  one fetch makes one `pr list` call: `tunnel.rs::a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once`.
- A detail read asks the forge for its parts together:
  `pull_requests.rs::a_detail_read_asks_the_forge_for_its_parts_together`.
- A request whose list read moves nothing skips the detail read; any one field moving, a posted comment awaiting confirmation, or a failed last detail read runs it again:
  `forge/live.rs::tests::unmoved_holds_where_the_list_read_repeats_the_held_fields`,
  `::unmoved_fails_where_any_one_field_moved`, `::unmoved_fails_with_no_details_or_failed_evidence`,
  `::unmoved_fails_when_held_details_expire`,
  `::unmoved_fails_while_a_local_addition_awaits_confirmation`,
  `::unmoved_fails_where_a_local_addition_landed_during_this_read`;
  `pull_request_detail_skip.rs::two_fetches_of_an_unmoved_request_run_one_detail_read`,
  `::a_fetch_whose_list_read_field_moved_runs_the_detail_read`,
  `::a_fetch_after_ariadne_posts_a_comment_runs_the_detail_read`,
  `::a_fetch_after_a_failed_detail_read_runs_the_detail_read_again`.
- A detail read drops the viewer's own pending review from the conversation
  outright — its own inline comment and a reply to it, both naming it by
  `pull_request_review_id`, give no entry of the list's own, so neither
  ever reaches `list_comments` nor wakes the agent that owns the request —
  while a submitted review's own comment stays:
  `forge/github/details.rs::tests::a_pending_reviews_body_comments_and_replies_are_dropped`,
  proved end to end through a real fetch in
  `pull_requests.rs::a_fetch_drops_the_viewers_own_pending_review_and_its_comments`,
  `kept_requests.rs::a_pending_reviews_comments_give_no_live_view_comment_and_no_news`.
  GitLab's detail read makes no call at all for the viewer's own draft
  notes to filter their content, since they never enter `discussions`
  until published, but still counts them to answer `has_pending_posts`:
  `pull_requests.rs::a_gitlab_fetch_counts_draft_notes_only_for_has_pending_posts`.
- `has_pending_posts` says whether the viewer's own pending review (GitHub)
  or draft notes (GitLab) are still open, so `merge_fresh_details` carries
  a review's own finding posted into that pending set past every poll
  until the viewer publishes, not only the one poll of grace any other
  local addition gets:
  `forge/github/details.rs::tests::a_pending_reviews_body_comments_and_replies_are_dropped`
  (`has_pending_posts` true while the review stays pending),
  `pull_requests.rs::a_gitlab_fetch_counts_draft_notes_only_for_has_pending_posts`
  (true while a draft note stays open),
  `forge/live.rs::tests::a_pending_reviews_finding_survives_a_poll_while_the_review_is_still_pending`,
  `::a_pending_reviews_finding_is_dropped_once_the_pending_review_is_gone`.
- Two reads of the list and of the search after one fetch make no further `pr list` call, a repository with no fetch yet lists empty with 200 and a disabled one still answers 409, the single-request route answers the held read with no `details()` call, and the panel route still reads live:
  `pull_request_lists.rs::two_reads_after_one_fetch_run_no_further_pr_list_call`,
  `::a_repository_with_no_fetch_yet_lists_empty_and_a_disabled_one_is_refused`,
  `::a_changed_forge_clears_the_held_list_before_its_fetch_succeeds`,
  `poll.rs::tests::a_stopped_worker_cannot_restore_its_held_list`,
  `::the_single_request_route_answers_the_held_read_with_no_details_call`,
  `::the_panel_route_still_reads_live`.
- An open author request kept by hand reads the forge once per cleanup wait,
  a request change reads inside that wait, and a failed read starts its own
  wait: `pull_request_keep_retry.rs::an_open_kept_request_is_read_once_per_wait`,
  `::a_changed_open_kept_request_is_read_inside_the_wait`,
  `::a_failed_kept_request_read_waits_before_its_retry`.
- A review request has a row while it is reviewed and none once it merged:
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
- Rate-limited Timer and WakeOnly fetches each pause the worker until the
  forge's reset time; a wake inside either pause fetches nothing, one fetch
  runs at the end, `fetch_error` reads the pause and then clears, and neither
  runs a signed-out confirmation after a rate-limit refusal:
  `forge_rate_limit.rs::a_rate_limited_timer_fetch_pauses_and_fetches_once_after_wakes_inside_it`,
  `forge_rate_limit.rs::a_rate_limited_wake_only_fetch_pauses_and_fetches_once_after_wakes_inside_it`.
  GitHub records only an exhausted resource's reset, falls back where no
  primary resource is exhausted, and pauses for one poll interval where that
  reset has passed:
  `github/mod.rs::tests::a_rate_limit_reset_uses_the_only_spent_resource`,
  `github/mod.rs::tests::a_rate_limit_reset_is_none_when_no_resource_is_spent`,
  `forge_rate_limit.rs::a_rate_limited_fetch_reads_the_only_spent_limit_reset`,
  `forge_rate_limit.rs::a_past_rate_limit_reset_waits_for_the_poll_interval`.
  A failed fetch that is not a rate limit doubles the timer wait up to the
  cap, resets on success, and a wake still fetches at once:
  `forge_rate_limit.rs::a_failed_fetch_in_timer_mode_backs_off_and_a_wake_still_fetches_at_once`.
  Wakes inside the debounce window fold into one fetch, and continued wakes
  do not move its first deadline:
  `forge_rate_limit.rs::three_wakes_inside_the_debounce_window_cause_one_fetch`,
  `forge_rate_limit.rs::continued_wakes_do_not_extend_the_debounce_window`.
  `forge/mod.rs::tests::a_refusal_is_rate_limited_on_either_forges_own_words`.
- GitLab reads fork clone URLs, checks, approvals, and reviewer roles; search matches number, author, or title:
  `pull_requests.rs::gitlab_requests_keep_fork_checks_review_and_search_by_author_or_number`.
- A fetch publishes `issues_changed` and `pull_requests_changed` only when they moved:
  `pull_requests.rs::a_fetch_publishes_issues_changed_only_when_the_open_issues_moved`,
  `::a_fetch_publishes_pull_requests_changed_only_when_the_requests_moved`.
- A thread waits on the login until it answers, a comment is told once, a reviewer hears of its own threads, and an Ariadne finding on my request is its author's while the answer is the review's:
  `live.rs::tests::a_thread_waits_on_the_login_until_it_answers_and_a_comment_is_told_once`,
  `::a_reviewer_hears_of_the_replies_in_its_own_threads`,
  `::an_ariadne_finding_on_my_request_is_the_authors_and_its_answer_the_reviews`.
- CLI actions use the pull request routes and preserve search text, `pr ls` asks every open request, mine, or the review requests, and no command adds or removes one:
  `pr.rs::tests::pr_commands_use_the_pull_request_routes_and_preserve_search_text`;
  `--mine` and `--review-requests` do not combine: `::mine_and_review_requests_do_not_combine`.
  `pr ls` carries the branches and the session, and `pr inspect` prints a
  fixed reading order with the body last: `::pr_ls_shows_the_branches_and_session_columns`,
  `::pr_inspect_prints_readable_keys_in_order_with_the_body_last`.
- `pr review` sends the asking pin whole and the stop body alone, is refused
  naming `--model` with no model and no `--stop`, and `--stop` conflicts
  with every asking flag:
  `pr.rs::tests::pr_review_sends_the_ask_body_with_its_pin_and_the_stop_body_alone`,
  `::pr_review_with_no_model_and_no_stop_is_refused`,
  `::pr_review_refuses_stop_combined_with_an_asking_flag_and_a_bare_default_model`.
- `session ls --pull-request` reaches the daemon under `SessionPageQuery`'s
  own parameter: `session.rs::tests::pull_request_reaches_its_own_query_parameter`.
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
- Start babysit and Stop babysit run a keeper session on the model and skills picked, open its console, and offer themselves on a request of mine that no task keeps alone:
  `pull-request-panel.test.tsx::starts Ariadne babysitting of a request of mine on the model and skills picked, opens its console, and stops it`,
  `::offers Start babysit on a request of mine that no task keeps, and on no other request`,
  `::offers no Start babysit on a request that asks for my review`,
  `::offers no Start babysit on a request of mine a task already keeps`.
- A row shows the Ariadne babysitting pill while a keeper session babysits it by hand:
  `pull-requests-page.test.tsx::shows the Ariadne babysitting pill on a row a keeper session babysits by hand`.
- Opening a task or session replaces the request panel and preserves list filters:
  `paths.test.ts::pull request panel navigation`.
- A new comment by another login is told once by id to the `pr` agent of the task that opened the request; a failed check is told by name; the request has no session of its own:
  `workflow_pull_request.rs::the_pr_column_opens_the_request_once_and_keeps_it`.
- The agent replies and reports; another session and no session get 403:
  `kept_requests.rs::the_pr_agent_replies_and_reports_and_no_other_session_may`.
- The keeping agent replies once to a conversation comment of the other side. A second reply to it gets 400 and makes no forge call. A reply to a comment of its own side also gets 400 and makes no forge call:
  `kept_requests.rs::the_pr_agent_replies_once_to_a_conversation_comment_of_the_other_side`.
- A trailer is stripped from the body of `open_pull_request`:
  `kept_requests.rs::open_pull_request_strips_a_trailing_trailer_from_the_body`.
- A repository that supervises its posts forces every opened request to a
  draft, whatever the agent asked, and one that does not leaves the agent's
  own value through unchanged:
  `kept_requests.rs::open_pull_request_forces_a_draft_when_the_repository_supervises_posts`,
  `::open_pull_request_leaves_the_agents_draft_value_unchanged_when_supervision_is_off`.
- A repository that supervises its posts holds a reply back: the route
  answers `pending: true` with the pending item's forge id, posts nothing to
  the forge, and the comment counts answered with no further reply taken;
  a repository that does not posts the reply as before:
  `kept_requests.rs::the_pr_agent_holds_its_reply_pending_when_the_repository_supervises_posts`,
  `::the_pr_agent_replies_and_reports_and_no_other_session_may`,
  `::the_pr_agent_replies_once_to_a_conversation_comment_of_the_other_side`.
- A reply to an inline review comment finds or creates the viewer's one
  pending review — created with the hidden verdict mark as its body — and
  joins its thread, reusing the same review on a second reply rather than
  creating another:
  `github/comments.rs::tests::a_pending_review_comment_reply_creates_a_pending_review_and_joins_its_thread`,
  `::a_second_pending_reply_reuses_the_same_pending_review`.
- A pending review found with no body, which GitHub refuses to edit, is
  deleted and made again with the hidden mark where it holds no draft
  comment, and is otherwise left to the user, the post refused saying so:
  `github/comments.rs::tests::a_bodiless_pending_review_holding_no_draft_is_deleted_and_made_again`,
  `::a_bodiless_pending_review_holding_drafts_is_the_users_to_settle`.
  `replace_pending_review_body` (rule 25) recovers the same bodiless
  review the same way, before replacing it with the caller's own new
  body outright, never left for GitHub to refuse the edit over:
  `github/comments.rs::tests::replace_pending_review_body_recovers_an_older_bodiless_review`.
- A reply to a top-level conversation comment quotes it, with a link to it,
  into the pending review's own body:
  `github/comments.rs::tests::a_pending_conversation_reply_quotes_the_comment_and_appends_to_the_pending_review`.
- GitLab posts a draft note on the comment's own discussion, never a
  published one:
  `gitlab/comments.rs::tests::a_pending_reply_posts_a_draft_note_on_the_comments_discussion`.
- A comment marked pending reads answered with no later comment on the
  forge to find it by:
  `live.rs::tests::a_pending_reply_marks_its_comment_answered_with_no_later_comment_on_the_forge`.
- A pending reply to one comment of a thread answers every earlier comment
  there too, and leaves a later one open:
  `live.rs::tests::a_pending_reply_answers_earlier_comments_of_its_thread_and_leaves_a_later_one_open`.
- The reply route announces the pending mark itself, since no forge fetch
  ever reads it back:
  `kept_requests.rs::the_pr_agent_holds_its_reply_pending_when_the_repository_supervises_posts`.
- The pending mark is written once and kept beside a mark of another kind
  on the same row:
  `store.rs::a_pending_reply_mark_is_written_once_and_kept_beside_others`.
- `reply_comment` tells the agent a pending reply waits in the user's forge
  drafts and to go on, rather than handing back the raw pending shape:
  `tools.rs::tests::a_pending_reply_tells_the_agent_to_continue_rather_than_the_raw_answer`.
- A supervised review holds new inline findings for the user to publish,
  while a supervised resolve is refused so the user resolves the fixed thread:
  `github/reviews.rs::tests::submit_review_pending_adds_each_finding_without_changing_the_body`,
  `pull_request_reviews.rs::a_review_resolves_its_own_fixed_finding_only_when_posts_are_not_supervised`,
  `tools.rs::tests::a_pending_review_tells_the_agent_to_continue_rather_than_the_raw_answer`.
- `edit_pending_comment` and `delete_pending_comment` change and remove a
  comment still in the viewer's own pending review, by its node id, and
  refuse one the review no longer holds; `replace_pending_review_body`
  overwrites the pending review's body outright, and refuses with no
  pending review (rule 25):
  `github/comments.rs::tests::edit_pending_comment_edits_a_comment_still_in_the_viewers_pending_review`,
  `::edit_pending_comment_refuses_with_no_pending_review`,
  `::edit_pending_comment_refuses_a_comment_the_pending_review_no_longer_holds`,
  `::delete_pending_comment_deletes_a_comment_still_in_the_viewers_pending_review`,
  `::replace_pending_review_body_replaces_the_whole_body_outright`,
  `::replace_pending_review_body_refuses_with_no_pending_review`.
- GitLab writes a draft note's edit and delete straight to its own id, and
  reads a 404 — a note the user already published — as no longer pending
  (rule 25):
  `gitlab/comments.rs::tests::edit_pending_comment_edits_the_draft_notes_body`,
  `::edit_pending_comment_refuses_a_draft_note_already_published`,
  `::delete_pending_comment_deletes_the_draft_note`,
  `::delete_pending_comment_refuses_a_draft_note_already_published`.
- The three routes answer 409 with no pending review or draft note to
  edit, and `edit_pending_review_body` replaces the pending review's body
  outright through the same mutation a reply's own write reuses (rule 25):
  `kept_requests.rs::pending_edit_routes_answer_409_with_no_pending_review`,
  `::edit_pending_review_body_replaces_the_pending_reviews_body`.
- Editing or deleting a held pending finding updates `list_comments` at
  once, never waiting on a detail fetch that would never read either
  change back on its own:
  `kept_requests.rs::editing_and_deleting_a_held_pending_finding_updates_list_comments_at_once`.
- A review GitHub creates fresh holds the hidden verdict mark as its body,
  never an empty one, so `edit_pending_review_body` can replace it right
  after without GitHub refusing the edit of a review it holds with no
  body at all:
  `github/comments.rs::tests::a_freshly_created_pending_review_is_editable_right_after`.
- A reply that folds its text into the pending review's body, rather than
  posting a comment of its own, brings `list_comments`'
  own copy of that body current too, read fresh off the forge right after
  (rule 25): neither folds its own answer back, so without this a later
  read would still answer the body the review held before either changed
  it:
  `github/comments.rs::tests::pending_review_body_reads_what_a_conversation_reply_just_folded_into_it`.
- A pending review past its first hundred comments still has a later one
  found: the node lookup `edit_pending_comment` and `delete_pending_comment`
  share pages the same way a request's threads do:
  `github/comments.rs::tests::edit_pending_comment_finds_a_comment_past_the_first_hundred`.
- A reply that folds into a pending review holding no comment of its own yet
  — no round of `submit_review_pending` ever posted one —
  still has `list_comments` answer its body right after: held through
  `add_comments`, which holds one whether or not it was already there,
  never `edit_comment`, which only changes one already held (rule 25).
  Marked `from_review`, the same mark `hold_review_comments` forces on
  `submit_review_pending`'s own first post of it, so the pending set it
  belongs to carries it past a poll the same way:
  `kept_requests.rs::a_reply_holds_the_pending_reviews_body_even_with_no_review_post_of_its_own`.
- A `forge_id` naming no comment or draft note of any shape — the
  caller's own mistake, not the forge's — answers 400, never the 409 a
  merely-submitted or merely-published one does, nor the 502 a real forge
  refusal does (rule 25):
  `github/comments.rs::tests::edit_pending_comment_refuses_a_forge_id_of_the_wrong_shape`,
  `gitlab/comments.rs::tests::edit_pending_comment_refuses_a_forge_id_of_the_wrong_shape`,
  `kept_requests.rs::pending_edit_routes_answer_400_with_a_malformed_forge_id`.
- A trailing trailer and the blank line before it are stripped, and text with no trailer is left whole:
  `forge/pulls.rs::tests::strip_trailers_takes_off_a_trailing_trailer_and_the_blank_line_before_it`,
  `::strip_trailers_leaves_text_with_no_trailer_whole`.
- The step stays on the `pr` column and its completion is refused until a merge, which is told to the agent; a quiet agent is then finished by the daemon and the row goes:
  `workflow_pull_request.rs::a_quiet_pr_agent_is_finished_after_the_merge`.
- A close is told to the agent and finishes nothing:
  `workflow_pull_request.rs::a_close_is_told_to_the_pr_agent_and_finishes_nothing`.
- A check that recovers and fails again is told again:
  `kept_requests.rs::a_check_that_recovers_and_fails_again_is_told_again`.
- The detail read reads every page of threads and check runs:
  `kept_requests.rs::the_detail_fetch_reads_every_page_of_threads_and_check_runs`.
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
- A ready report moves once, per head:
  `store.rs::a_pull_request_reports_ready_once_per_head`.
- The agent seat lists the request's tools beside its task tools:
  `mcp.rs::tests::the_agent_seat_lists_its_step_tools_and_nothing_of_a_review`.
- Its request tools find the request its task opened, then call its routes, and say to open one where there is none:
  `tools.rs::tests::the_agents_request_tools_call_the_routes_of_the_request_its_task_opened`,
  `::an_agent_with_no_request_is_told_to_open_one`.
- The skill is within its caps, names no forge CLI, timer, poll or resolve, opens the request once filling the repository's template or its three sections, starts on the news steps where one is already open, and ends on the merge or the close:
  `defaults.rs::tests::the_pr_babysit_skill_opens_the_request_and_ends_on_the_merge_or_the_close`, `::skill_size_caps_hold`.
- The skill answers each inline review comment once, in one of three shapes, skips a thanks, acts on a follow-up, never retries once a reviewer reply keeps a finding open, answers a conversation comment the same way, applies an accepted suggestion block, and posts no unrelated reply:
  `defaults.rs::tests::the_pr_babysit_skill_answers_each_comment_once_in_its_shape`.
- `session ls` titles a request session with its title and URL:
  `session.rs::tests::a_pull_request_session_shows_the_request_title_and_url`.
- A ready report raises no row of its own on `ariadne attention`: that is
  the `pull_request` attention producer's own item, confirmed against the
  forge's evidence, not a bare flag on the session (031):
  `board.rs::tests::a_pull_request_sessions_own_waiting_user_raises_no_row_of_its_own_on_this_board`.
- The Pull requests screen row opens its session and shows the unanswered count:
  `pull-requests-page.test.tsx::opens a request's session panel from its row and shows its unanswered comments`.
- A session event of a request refetches that request:
  `dispatch.test.ts::refetches the request a session watches when that session starts or moves, for its session_id`,
  `::leaves the pull requests alone for a session that watches none`.
- `pull_requests_changed` reads every request key again:
  `dispatch.test.ts::reads every request again on pull_requests_changed, which carries none`.
- The session listing takes `pull_request` in OpenAPI:
  `session_list.rs::the_query_and_the_page_are_in_the_openapi_document`.
- A `502 forge_error` is never retried, while another `502` still gets its retries:
  `queries.test.ts::createQueryClient's retry > never retries a 502 whose body error is forge_error`,
  `::still retries another 502`.

## Known gap

GitHub's `reviewRequests` names a team by a slug or a name, never a login.
A review asked of a team the user belongs to is read as asking nobody, so it
raises no row and starts no session (029).

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/src/pull_requests.rs`, `crates/ariadne-store/src/pull_request_comments.rs`,
`crates/ariadne-store/skills/pr-babysit/SKILL.md`, `crates/ariadne-api/src/pull_requests.rs`,
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-cli/src/commands/mcp.rs`, `crates/ariadne-cli/src/commands/mcp/tools.rs`,
`crates/ariadne-daemon/src/forge/`, `crates/ariadne-daemon/src/forge/live.rs`,
`crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/http/channel.rs`, `crates/ariadne-daemon/src/bus.rs`,
`crates/ariadne-cli/src/commands/pr.rs`, `ui/src/features/pull-requests/`,
`ui/src/api/queries.ts`, `.github/pull_request_template.md`.
