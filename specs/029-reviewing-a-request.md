---
id: reviewing-a-request
status: current
updated: 2026-10-11
areas: [store, api, daemon, cli, mcp, prompts, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/pull_request_reviews.rs
  - crates/ariadne-daemon/tests/it/kept_requests.rs
  - crates/ariadne-daemon/tests/it/workflow_pull_request.rs
  - crates/ariadne-daemon/tests/it/attention.rs
  - crates/ariadne-daemon/src/forge/news.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/src/commands/attention/board.rs
  - crates/ariadne-cli/src/commands/pr.rs
  - ui/src/features/pull-requests/pull-requests-page.test.tsx
  - ui/src/features/pull-requests/pull-request-panel.test.tsx
---

# Reviewing a request

Every open request where the user is a requested reviewer gets a session of
its own, and so does a request of the user's own whose review they asked
Ariadne for. The daemon wakes it with the request and with each push. It
posts one review in the user's name, and the approval stays the user's.

## Scope

In: which rows want a review session, the asking on a request of the
user's own, its worktree, its news, its tools and routes, the verdict
policy, and the attention it raises.

Out: the rows Ariadne keeps, the fetch, the live reads and the PR session kind
([026](026-pull-requests.md)); the skill catalog and staffing
([017](017-skills-and-staffed-agents.md)); the tool surface as a whole
([013](013-mcp-tool-surface.md)).

## Behavior

1. A request that asks for the user's review, out of draft, gets a row with
   role `reviewer` on the first fetch that finds it (026 rule 3), whether or
   not its repository's integration names a `review_model`: the row is what
   lets a human still start one by hand where nothing else will (031's
   `pull_request` producer). That row wants one live session while the
   request is `open`, not a draft, still asks for the user's review, and
   either the repository's integration names a `review_model` or the row
   itself was given one directly (rule 10a). The daemon staffs it on that
   pin — the repository's own, or the row's own where the repository names
   none — with seat `reviewer` and the `pr-reviewer` skill (017). A row with
   role `author` wants one where `review_asked` holds: the user asked
   Ariadne to review a request of their own (rule 10). That session runs on
   the pin the user picked, `pull_requests.review_model` and
   `review_effort`, and loads `pr-reviewer` and the skills they picked,
   `review_skills`; the repository's `review_model` has no part in it.
2. A draft starts nothing. When the request leaves draft, the next fetch
   starts the session. A request that goes back to draft ends it.
3. Whether the request still asks for the user's review is read on every
   fetch and held with the live read, never stored (026 rule 4). The fetch's
   list of review requests that holds it says yes (026 rule 5). For an open
   row that asked on the last read and the list no longer holds, the fetch
   asks the forge
   (`ForgeClient::review_still_requested`). A detail read with no fetch
   before it says the request still asks: only a fetch withdraws it. GitHub stops listing a request
   once the user reviewed it, and writes no removal event for that. So it
   reads the request's timeline: the last review request or removed request
   for the login decides. A review decides nothing, so a review posted after a
   withdrawal does not ask again. GitLab keeps a reviewer listed after a review,
   so a request it does not list asks for nothing. A review of the user's own
   keeps nothing alive. Where the forge cannot answer, the last read stands.
4. The worktree is `worktree_root/pr-<id>`, detached at `head_sha`
   (`Launcher::review_worktree`), as a task reviewer's is at its branch tip
   (004). A head the checkout lacks is fetched into `FETCH_HEAD` from
   `head_repo`, else from the integration's remote: the head branch may share
   its name with a branch of the checkout. No local branch is written.
5. The briefing names the repository, the worktree and its head, the
   branches, the login, and the last `reviewed_sha`, or `none`. The seat text
   says to work only in the detached worktree, and to commit and push
   nothing.
5a. A review session that went away is resumed on its own row and
   conversation, unless the pin moved since it started: a conversation keeps
   the model it started on, so a new model or effort is a fresh session on
   it.
6. `pull_requests.told_head_sha` is the head the session was last told of.
   Each start and resume writes it to `head_sha`, since the briefing names
   that head.
7. The news of a reviewer row (`forge/news.rs`) is a push and the replies in
   the threads the integration login opened, each told once (026 rule 14).
   A push is a `head_sha` that differs from `told_head_sha`. Its line names
   the new head and the last `reviewed_sha`, and asks for a review of the
   commits since it. Checks, the base, the review decision and the state are
   no news to a reviewer. On a request of the user's own a push alone is
   news: its comments are the news of the task's author that keeps it (026),
   and each is told once.
8. A review's news is told at once: the next reconcile after a push or a
   reply hands it over, with no wait for activity to settle. A push also
   waits for the session to be idle: once it is, the daemon moves the
   worktree to the new head (`checkout_detached`) and hands the news in one
   prompt. A fetch with the same head hands nothing. A reply alone needs no
   idle session, since it moves no worktree:
   `pull_request_reviews.rs::a_push_reaches_an_idle_session_without_a_wait`.
9. A row that turns `merged` or `closed`, goes back to draft, or loses the
   user's review request has its sessions killed and its worktree removed. No
   branch is touched. Then Ariadne stops working on it and its row goes,
   but for a draft, whose review waits for it to leave draft.
   A request of the user's own leaves its row to its author's side; one
   whose asking stopped and that no task keeps goes as well.

## Asking on a request, by hand

10. `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-review`
    takes `{asked, model, effort, skills}`. Asking starts Ariadne's work on
    the request, writes `pull_requests.review_asked`, the pin
    `review_model` and `review_effort`, and `review_skills`. `model` is
    required to ask and is checked against the catalog as any pin is; a
    missing or unknown one answers 400. Each skill must be one a task agent
    is staffed on (017), else 400; `pr-reviewer` is loaded anyway and is not
    stored. It answers the request, and wakes the fetch and the scheduler.
    A request of the user's own has its row written with role `author`
    where asking is the first thing to give it one. A request that asks for
    the user's review is refused with 409 only where its repository already
    pins a `review_model` of its own: that row already has a review of its
    own, on the repository's pin, and this ask would run a second, redundant
    one beside it. A repository with no pin of its own takes the ask on a
    request that asks for the user's review exactly as it takes one on a
    request of the user's own: nothing else is ever going to start that
    session (031's `pull_request` producer is what tells a human this one
    wants it). Asking on an ended request is refused with 409. `asked:
    false` clears the pin and the skills and takes the session down.
10a. `pr review <repo> <number> --model AGENT:MODEL [--effort E] [--skill
    S]... [--attach]` and `pr review <repo> <number> --stop` call this route
    from the CLI (014), on a request of the user's own or on one that asks
    for the user's review alike: `--model` left out takes the repository's
    own `review_model` and `review_effort` where it has one, and `--attach`
    waits for the review session this starts and opens its console.

## Tools and routes

11. The reviewer PR seat lists `get_pull_request`, `get_diff`,
    `list_comments`, `reply_comment`, `resolve_thread`, `submit_review` and
    `report_pull_request`, and no task tool and no message tool (013). The
    user reaches the session through its console (008).
12. `GET /v1/pull-requests/{id}/diff?since=` answers `git diff <base>...HEAD`
    in the session's worktree, or `git diff <since>..HEAD` where `since` is
    given. The base is `<remote>/<base_branch>` where the checkout holds it,
    else the local base branch. A `since` that is no hex sha of a commit in
    the worktree answers 400.
13. `POST /v1/pull-requests/{id}/reviews` takes `event`, `body` and
    `comments` of `path`, `line`, `title`, `body` and `priority` (`P0`, `P1`
    or `P2`). One call is one round of the review. `comments` are the
    round's new findings, each one inline comment on the line of its
    defect, posted as `**[P0] Title**`, a blank line, then its body: what
    goes wrong and how to fix it. `body` is required: the review's whole
    summary as it stands, tied to no line. The review keeps one summary
    comment on the request (`pull_requests.summary_comment_id`):
    the first round posts it, and every later round replaces its
    text, so the request carries one summary however many rounds ran. A
    summary the forge answers 404 for is posted again; any other failed
    edit is the error, and posts no second summary. Every comment a review
    posts — each finding, the summary, each reply — ends on an HTML comment,
    `<!-- ariadne:review -->`, which the forge renders as nothing, and the
    summary on `<!-- ariadne:review-summary -->` too: what tells the
    review's comments from the user's own when no row recalls them. A review
    stopped and asked again, whose row went in between, finds its findings
    and edits its summary by them. The marks are left out of every body a
    reader is shown. The findings are marked and held before the summary is
    written: a summary that fails answers 502 saying the findings are
    posted, and a round with no comments writes it. A round with no new
    finding posts no review, only the summary. A comment with no path, no
    line from 1, no title or no body answers 400, and so does an empty
    `body`, and a `request_changes` with no P0 among its comments and none
    of the review's still open: a change request shows its P0s on their
    lines.
    - GitHub: one `gh api repos/<owner>/<name>/pulls/<n>/reviews` call with
      `REQUEST_CHANGES` or `COMMENT`, the head as `commit_id`, every inline
      comment and no body; a forge whose own answer says the review lacks a
      body takes it again with one line that points at the summary. A forge
      that refuses answers 502 with its own reason — GitHub's
      `{"message", "errors"}` beside the CLI's line — so the session can act
      on it, and the skill says the failure rather than posting another way. The daemon then
      reads the review's comments back. The summary is an issue comment,
      `.../issues/<n>/comments`, edited with `PATCH
      .../issues/comments/<id>`.
    - GitLab: one discussion per inline comment on the merge request's diff,
      through `glab api`. GitLab has no review verdict: the summary note,
      edited with `PUT .../notes/<id>`, says where the review stands. Each
      position names the file on both sides of the diff: the merge
      request's `diffs` are read before anything is posted, and a renamed
      file takes its old path as `old_path`.
    The daemon marks the posted summary and comments `from_review`
    (`pull_request_comment_marks`, 026 rule 10), as it marks a review
    session's replies, holds them beside the last read until the next fetch
    reads them back, and answers them with 201. On a request of
    the user's own a `from_review` comment counts as another login's: it is
    told once to the task's author, as "by the Ariadne review", and waits
    on it until the author replies (026 rules 12 to 16). The other way, a
    comment under the login that no review posted — the author's answer,
    or the user's — in a thread a review opened is the review session's
    news, so the answer to a finding reaches its reviewer. On a request of the user's
    own the review is a comment whatever its `event`: no forge takes a
    change request from a request's own author.
14. Every `event` but `request_changes` and `comment` answers 400 before the
    forge sees a call. An approval is refused above all: the user gives it.
    No route approves or merges.
15. `report_pull_request` takes `reviewed_sha`, a hex sha, on a reviewer row,
    or on a row of the user's own with `review_asked`, alone, and `ready`,
    whether the babysitting task believes every required approval and
    check reads green. Both are written — `pull_requests.reviewed_sha` and
    `pull_requests.ready` — and neither raises anything of its own on the
    session: a pr-reviewer session finishing its review must notify
    nobody, and a `ready` claim alone is not confirmed evidence. The
    `pull_request` attention producer (031) reads the forge's own evidence
    against the request's current state instead of trusting either call.
16. Diffs, reviews and reports are accepted from the request's own session
    alone. Another session, or a call with no session, gets 403.
16a. `POST /v1/pull-requests/{id}/comments/{comment_id}/resolve` resolves
    the thread of that comment on the forge, and fetches the repository
    again, which reads it back resolved. Only the
    request's review session calls it, and only on a thread whose first
    comment is by the integration login: a thread anyone else opened
    answers 403, and so does the author of the task that opened the
    request. A resolved thread answers as it is, with no forge call.
    - GitHub: `resolveReviewThread` through `gh api graphql`, on the thread's
      node id. A comment posted since the last detail fetch has its own id
      for a thread, so the request's threads are read for it first.
    - GitLab: `PUT .../merge_requests/<n>/discussions/<id>` with
      `resolved=true`, through `glab api`.
    The conversation is no thread, and nothing resolves it.

## Verdict and attention

17. `pr-reviewer` asks for changes when a P0 finding exists, and comments
    otherwise. P0 breaks behavior, data or security and must change before
    the request lands; P1 is a defect or a missing proof that will bite; P2
    is worth fixing. It writes the review body as the whole summary of the
    review as it stands, in three parts, with no file, no line and nothing
    of what it did:
    - a header, one line per commit the round reviewed, each a short sha
      and its subject, read with `git log <base>..HEAD` in the worktree on
      the first round, or `git log <reviewed_sha>..HEAD` later, and the
      full base-to-head range on its own line;
    - a prose summary, not bullets, of what the change does and what the
      open findings mean for it, naming each open finding's priority and
      title;
    - a recommendation of one line: "Request changes" and the blocking P0
      findings while one is open; "Changes recommended" and what to fix
      before it lands while a P1 or P2 is open and no P0 is; else "No
      findings: ready for a human to approve".
    Each new finding is one inline comment with a title, the failure and a
    fix. On a later round it posts nothing more in a thread nobody answered
    since its last entry: it waits for the answer. A thread a commit fixed
    gets one reply and is resolved; an answered thread whose defect is
    still there gets one reply that says so and why.
18. A review posted, or a request reported ready, notifies nobody by
    itself: the approval and the merge stay the user's to give in their own
    time, and only the `pull_request` attention producer's own readiness
    item (031), confirmed against the forge's own evidence, tells the user
    a request of theirs is actually ready to merge. A pr-reviewer session's
    own completed review raises no notification, no attention row and no
    badge increment of any kind.

## Acceptance criteria

- An open request out of draft gets one live session on the review pin, seat
  `reviewer`, detached at `head_sha`, with `pr-reviewer` indexed:
  `pull_request_reviews.rs::an_open_request_i_review_gets_one_session_detached_at_its_head`.
- The review session resolves the thread of its own fixed finding through
  `resolveReviewThread`, and a thread someone else opened, or a resolve from
  another session, is refused:
  `pull_request_reviews.rs::a_review_resolves_the_thread_of_its_own_fixed_finding_and_no_other`.
- A round's findings are one review with no body, the summary is one
  comment posted once and edited by the next round, a round with no new
  finding posts no review, and a change request stands on a P0 still open:
  `pull_request_reviews.rs::a_review_posts_its_findings_by_priority_and_an_approval_is_refused`.
  GitLab's summary note is edited the same way:
  `pull_request_reviews.rs::a_gitlab_finding_on_a_renamed_file_names_its_old_path`.
  The author's answer in a review's thread reaches the review:
  `kept_requests.rs::an_ariadne_review_of_a_kept_request_reaches_its_pr_agent`.
- A summary that fails keeps the findings posted, and a round with no
  comments writes it:
  `pull_request_reviews.rs::a_failed_summary_keeps_the_posted_findings_for_a_round_with_no_comments`.
  A failed summary edit posts no second summary; one GitHub says is gone is
  posted again:
  `pull_request_reviews.rs::a_summary_is_posted_again_only_where_github_says_it_is_gone`.
  A detail read before any fetch withdraws no review request:
  `pull_request_reviews.rs::a_detail_read_before_any_fetch_keeps_a_review_request_asking`.
  A review stopped and asked again edits its summary, found by its mark:
  `pull_request_reviews.rs::a_request_of_mine_is_reviewed_once_asked_and_its_review_is_a_comment`.
- Back-to-back pushes each reach an idle review session on the next
  reconcile, with no wait for the news to settle:
  `pull_request_reviews.rs::a_push_reaches_an_idle_session_without_a_wait`.
- On GitLab a finding on a renamed file names its old path:
  `pull_request_reviews.rs::a_gitlab_finding_on_a_renamed_file_names_its_old_path`.
- An Ariadne review of a request a task's author keeps reaches that author,
  waits on it, and is answered by its reply:
  `kept_requests.rs::an_ariadne_review_of_a_kept_request_reaches_its_pr_agent`.
- A draft is no work and gets no row until it leaves draft:
  `pull_request_reviews.rs::a_draft_starts_no_review_until_it_leaves_draft`.
- A repository with no `review_model` of its own starts no session for a
  request that asks for the user's review, though the row it needs to
  offer a manual start still exists:
  `pull_request_reviews.rs::a_repository_with_no_review_model_starts_no_review`.
- A push moves the worktree and hands one prompt that names the last
  `reviewed_sha`; the same head again hands nothing:
  `pull_request_reviews.rs::a_push_moves_the_worktree_and_is_told_once_with_the_last_reviewed_sha`,
  `news.rs::tests::a_push_to_a_request_i_review_is_told_once_with_the_last_reviewed_sha`.
- `get_diff` answers the diff against the base, and `since` narrows it:
  `pull_request_reviews.rs::the_diff_reads_the_worktree_against_its_base_and_since_narrows_it`.
- A review with `request_changes` and two comments runs the forge's review
  call with `REQUEST_CHANGES`, both comments and their priorities, and marks
  them the review's; `approve` answers 400 and the forge sees no call:
  `pull_request_reviews.rs::a_review_posts_its_findings_by_priority_and_an_approval_is_refused`.
- A `reviewed_sha` is stored and raises no `waiting_user`; the same sha
  again, and a later one, raise nothing either:
  `pull_request_reviews.rs::a_reviewed_sha_is_stored_and_raises_no_waiting_user`,
  `store.rs::a_reviewed_sha_moves_once`.
- A merged, closed or withdrawn request ends the session, removes the
  worktree, and Ariadne stops working on it:
  `pull_request_reviews.rs::a_merged_request_ends_its_review`,
  `::a_closed_request_ends_its_review`,
  `::a_withdrawn_review_request_ends_its_review`,
  `::a_request_withdrawn_after_my_review_ends_its_review`,
  `::a_review_posted_after_a_withdrawal_does_not_keep_the_review`.
- A request the forge stops listing after my review keeps its session while
  its timeline ends on my review:
  `pull_request_reviews.rs::a_request_the_forge_stops_listing_after_my_review_keeps_its_review`.
- The reviewer briefing fills every placeholder it names:
  `prompts.rs::tests::the_pull_request_texts_fill_every_placeholder_they_name`.
- The reviewer PR seat lists its eight tools and no task or message tool, and
  its tools call the routes of its request:
  `mcp.rs::tests::the_pull_request_reviewer_seat_lists_its_tools_and_no_task_or_message_tool`,
  `tools.rs::tests::the_pull_request_reviewer_tools_call_the_routes_of_the_sessions_request`.
- The skill is within its caps, names the three priorities, names
  `request_changes` only beside P0, resolves a thread a push fixed, and
  approves on its own no more than it names merge, sleep, poll, `gh` or
  `glab`:
  `defaults.rs::tests::the_pr_reviewer_skill_ranks_its_findings_and_never_approves`,
  `::skill_size_caps_hold`.
- The summary is a header read from local history, then a prose paragraph,
  then a recommendation, in that order:
  `defaults.rs::tests::the_pr_reviewer_skill_writes_its_summary_as_a_header_then_prose_then_a_recommendation`.
  The prose names what the change does and where and how serious its risk
  is, and names every open finding in it, not as a bullet:
  `defaults.rs::tests::the_pr_reviewer_skill_summarizes_the_change_and_its_risk_in_prose`.
  Each recommendation line is tied to the priority that earns it:
  `defaults.rs::tests::the_pr_reviewer_skill_recommends_by_the_open_findings_priority`.
- A request of mine is no work and gets no review until the user asks; the
  ask starts the work, and the review runs on the pin and with the skills
  they picked, detached at its head, and is a comment; stopping the asking
  ends it and the row, its session let go of it; asked again on another
  model, the review is a fresh session on that model:
  `pull_request_reviews.rs::a_request_of_mine_is_reviewed_once_asked_and_its_review_is_a_comment`.
- Asking needs a model the catalog holds and skills a task agent is staffed
  on, and a refusal starts no work. Asking is taken, not refused, on a
  request that asks for my review where the repository pins none of its
  own, the same as on a request of mine; asking the same request again
  once the repository pins one of its own is refused:
  `pull_request_reviews.rs::asking_needs_a_model_and_takes_an_unpinned_review_request`.
- A review's findings are inline comments titled `[Pn] Title`; a change
  request with no P0 inline and a comment with no title are refused:
  `pull_request_reviews.rs::a_review_posts_its_findings_by_priority_and_an_approval_is_refused`.
- The desktop's request panel starts an Ariadne review of a request of mine
  on the model and skills picked in its dialog — `pr-reviewer` always on and
  nothing else picked to start with — closes the dialog and opens the new
  review session's console in the panel once the daemon starts it, stops it,
  and offers it on no other:
  `pull-request-panel.test.tsx::starts an Ariadne review of a request of mine on the model and skills picked, opens its console, and stops it`,
  `::offers no Ariadne review on a request that asks for my review when the repository already pins one`. A review
  resumed on its last session opens that console the same way:
  `::opens the console of a review resumed on the same session`. A request
  that asks for the user's review, on a repository with no pin of its own,
  offers the same manual start as a request of the user's own:
  `::offers a manual Start review on a request that asks for my review when the repository pins none`.
- Neither a posted review nor a `ready` report raises a row, a toast or a
  badge of its own: a pull request session's own `waiting_user` carries no
  reason this board, the strip or the sessions screen's filter reads any
  more
  (`attention/board.rs::a_pull_request_sessions_own_waiting_user_raises_no_row_of_its_own_on_this_board`).
- `ariadne pr review` sends the pin it is given or the repository's own, is
  refused with no pin at all, and `--stop` sends `asked: false` alone and
  conflicts with every asking flag:
  `pr.rs::tests::pr_review_sends_the_ask_body_with_its_pin_and_the_stop_body_alone`,
  `::pr_review_with_no_model_uses_the_repositorys_own_review_pin`,
  `::pr_review_with_no_model_and_no_repository_pin_is_refused`,
  `::pr_review_refuses_stop_combined_with_an_asking_flag_and_a_bare_default_model`.

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/skills/pr-reviewer/SKILL.md`,
`crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-daemon/src/forge/news.rs`, `crates/ariadne-daemon/src/forge/poll.rs`,
`crates/ariadne-daemon/src/forge/github/reviews.rs`,
`crates/ariadne-daemon/src/forge/gitlab/reviews.rs`,
`crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-daemon/src/attention/pull_requests.rs`
(031), `crates/ariadne-cli/src/commands/mcp.rs`, `crates/ariadne-cli/src/commands/mcp/tools.rs`,
`crates/ariadne-cli/src/commands/pr.rs`, `ui/src/features/sessions/session-display.tsx`,
`ui/src/features/pull-requests/pull-request-panel.tsx`.
