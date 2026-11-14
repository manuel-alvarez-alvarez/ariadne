---
id: reviewing-a-request
status: current
updated: 2026-11-13
areas: [store, api, daemon, cli, mcp, prompts, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/pull_request_reviews.rs
  - crates/ariadne-daemon/tests/it/kept_requests.rs
  - crates/ariadne-daemon/tests/it/workflow_pull_request.rs
  - crates/ariadne-daemon/tests/it/attention.rs
  - crates/ariadne-daemon/src/forge/news.rs
  - crates/ariadne-daemon/src/forge/github/reviews.rs
  - crates/ariadne-daemon/src/forge/github/comments.rs
  - crates/ariadne-daemon/src/forge/pulls.rs
  - crates/ariadne-daemon/src/forge/github/pulls.rs
  - crates/ariadne-daemon/src/forge/pulls.rs
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

Any open request, regardless of its author or assigned reviewers,
gets a review session once the user asks Ariadne for one — never on its
own. The ask makes one goal with one staffed task, and the review session is
that task's agent (rule 10b). The daemon wakes it with the request and with
each push. It posts one review in the user's name, and the approval stays
the user's.

## Scope

In: which rows want a review session, the manual ask on any request, the
goal and task it makes, its worktree, its news, its tools and routes, the
verdict policy, and the attention it raises.

Out: the rows Ariadne keeps, the fetch, the live reads and the PR session kind
([026](026-pull-requests.md)); the skill catalog and staffing
([017](017-skills-and-staffed-agents.md)); the tool surface as a whole
([013](013-mcp-tool-surface.md)).

## Behavior

1. Any open request can be reviewed once explicitly asked through rule 10,
   whether its author is the user, another person or a bot, and whether
   GitHub assigned the user a review. Its row wants one live session while
   `review_asked` holds, the request is open, draft or not, and the forge
   integration is enabled. The session runs on `review_model` and
   `review_effort`, with seat `reviewer`, `pr-reviewer` and the picked
   `review_skills`, on the task of the review goal (rule 10b). A GitHub review request still creates a row on the
   first fetch so attention can offer a manual start (026, 031); it never
   starts a session by itself.
2. A draft is no work of its own: a review request on one creates no row
   until it leaves draft. An explicit ask on a draft starts the session at
   once, since the ask is the user's word that the review is wanted now;
   leaving draft or going back to it neither starts nor ends that session.
3. Whether the request still asks for the user's review is read on every
   fetch and held with the live read, never stored (026 rule 4). The fetch's
   open list says yes where it names the login among the request's own
   requested reviewers (026 rule 5). For an open row that asked on the last
   read and the list no longer names the login, the fetch asks the forge
   (`ForgeClient::review_still_requested`). A detail read preserves the
   cached assignment. With no cached read, a held `reviewer` row still
   asks until a fetch checks the list and timeline. This default starts
   no session without an explicit ask. GitHub
   drops the login from `reviewRequests` once the user reviewed it, and
   writes no removal event for that. So it reads the request's timeline: the
   last review request or removed request for the login decides. A review
   decides nothing, so a review posted after a withdrawal does not ask
   again. This field controls review-request attention, not the explicit
   manual review: withdrawing an assignment does not stop that session.
   Where the forge cannot answer, the last read stands. A review
   asked of a team the user belongs to is never found this way, since
   GitHub names a team by a slug or a name, never a login (026 Known gap).
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
   no news to a reviewer. On a request of the user's own or one with a
   keeper (032), findings are the keeper's news and replies from the integration login
   in the review's threads are the reviewer's news. Each is told once.
   The viewer's own pending review, its body, its comments and its replies
   give a reviewer row none of this: the detail read drops them from the
   conversation outright (026 rule 9), so neither this row's own news nor
   its author's ever names one, and nothing here starts a review pass over
   content nobody has published yet
   (`pull_request_reviews.rs::a_pending_review_gives_the_reviewer_session_no_live_view_comment_or_news`).
8. A review's news is told at once: the next reconcile after a push or a
   reply hands it over, with no wait for activity to settle. A push also
   waits for the session to be idle: once it is, the daemon moves the
   worktree to the new head (`checkout_detached`) and hands the news in one
   prompt. A fetch with the same head hands nothing. A reply alone needs no
   idle session, since it moves no worktree:
   `pull_request_reviews.rs::a_push_reaches_an_idle_session_without_a_wait`.
9. A request that turns merged or closed, or whose manual ask is stopped,
   has its reviewer session killed and its detached worktree removed. A
   merge completes the review goal and its task; a close fails the task and leaves the goal active; a stop cancels
   both (rule 10b).
   Stopping review preserves an active keeper or task; stopping a keeper
   preserves an asked review. The request's row is removed when no work or
   outstanding review-request attention remains. GitHub assignment changes
   alone never stop an explicitly asked review.

## Asking on a request, by hand

10. `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-review`
    takes `{asked, model, effort, skills}`. Asking starts Ariadne's work on
    the request, writes `pull_requests.review_asked`, the pin
    `review_model` and `review_effort`, and `review_skills`. `model` is
    required to ask and is checked against the catalog as any pin is; a
    missing or unknown one answers 400. Each skill must be one a task agent
    is staffed on (017), else 400; `pr-reviewer` is loaded anyway and is not
    stored. `model` must have a rank the `pr-review` workflow's `review`
    column allows (030 rule 11): an unranked model, or one of another rank,
    answers 400 naming the band — the band of the open goal's snapshot where
    one is open, else of the workflow as it stands. It answers the request,
    and wakes the fetch and the scheduler.
    Authorship and GitHub review assignments do not restrict the ask.
    The row keeps its actual authorship role: `author` for the integration
    login, otherwise `reviewer`; starting a session does not invent a
    GitHub assignment. Asking a second time on a request that already has
    one moves its pin and its skills onto the new ask, rather than refusing it. Asking on
    an ended request is refused with 409. `asked: false` clears the pin and
    the skills and takes the session down.
10a. `pr review <repo> <number> --model AGENT:MODEL [--effort E] [--skill
    S]... [--attach]` and `pr review <repo> <number> --stop` call this route
    from the CLI (014), on any request: `--model` is required with no
    `--stop`,
    since there is no repository pin to fall back to any more, and names
    the flag when it is missing. `--attach` waits for the review session
    this starts and opens its console.

10b. A review ask makes one active goal of the review operation, and a
    stop or an end finishes it. The lifecycle is shared by every PR
    operation (`Store::start_pr_operation`, `Store::end_pr_operation`); the
    babysit operation applies the same one (032 rule 7b).
    - One open goal exists per request and operation, named by value in
      `goals.pr_operation`, `pr_repository_id`, `pr_number` and `pr_url`: the
      request's own row goes when Ariadne stops working on it, and the goal
      stays as history. A partial unique index keeps the one.
    - The goal runs on `pr-review`, snapshotted into `goal_steps` as any
      goal's workflow is (030 rule 1), is `active` at once and has no
      orchestrator (`orchestrated` is false). It has one task, already
      `in_progress` on the `review` column, and one agent: the pin of the
      ask and the column's skills with the picked ones. No orchestrator
      plans it, and no task pass runs it: the request's own pass starts,
      resumes and watches its session (rule 1), and the session carries the
      goal, the task and the agent, so the Goals board, the task panel, the
      session panel, its history and its usage read it as any task's.
    - A repeated ask keeps the open goal, its task and its agent, and moves
      the pin and skills onto the agent. The live session runs on, and the
      next start uses the new pin.
    - A merge completes the task and the goal; a close fails the task and leaves the goal active; a stop, a goal
      whose ask is gone, or an integration turned off cancels both. The next
      ask after a stop makes a new goal and a new session: a session
      belongs to the goal it started under, and one from before goals is
      not adopted.
    - One lock per request (`Store::pr_operation_lock`) serializes a start,
      a stop and the part of a scheduler pass that acts on the goal. The
      pass rereads the row under the lock, so it never reads a start whose
      goal is made and whose ask is not, nor a stop half done. A start
      writes the goal and then the ask; a stop clears the ask and then
      cancels the goal (`Store::stop_pr_operation`), so no pass starts a
      session for an ask nobody owns. A pass still ends a goal whose ask is
      gone, whose task was ended by hand, or whose request ended, so a
      failed write converges. Starting twice at once makes one goal and one
      task.
    - Cancelling the goal from the Goals board (`POST /v1/goals/{id}/cancel`)
      is a stop: it clears the ask and cancels the goal and its task under
      the same lock, so Forge shows the review stopped and no session
      starts afterwards. The goal is read again under the lock: one a stop
      and a new start replaced while the cancel waited answers 409, and the
      replacement stays.
    - `GoalDto.pull_request`, `TaskDto.operation` and
      `PullRequestDto.operations` name the operation, the request, and the
      goal and task of each open one; the desktop's request panel links the
      goal, and `pr show` prints it.

## Tools and routes

11. The reviewer PR seat lists `get_pull_request`, `get_diff`,
    `list_comments`, `reply_comment`, `resolve_thread`, `submit_review` and
    `report_pull_request`, and no task tool and no message tool (013). The
    user reaches the session through its console (008).
12. `GET /v1/pull-requests/{id}/diff?since=` answers `git diff <base>...HEAD`
    in the session's worktree, or `git diff <since>..HEAD` only where `since`
    is an ancestor of the current head. The base is `<remote>/<base_branch>`
    where the checkout holds it, else the local base branch. A `since` that is
    not a hex sha of a commit in the worktree, or is not an ancestor, answers
    400 so rewritten history receives the full diff.
13. `POST /v1/pull-requests/{id}/reviews` takes `event`, an optional legacy `body`, and
    `comments` of `path`, `line`, `title`, `body` and `priority` (`P0`, `P1`
    or `P2`). One call is one round of the review. `comments` are the
    round's new findings, each one inline comment on the line of its
    defect, posted as `**[P0] Title**`, a blank line, then its body: what
    goes wrong and how to fix it, with a trailer an agent's skill appends
    stripped first (026 rule 16). The daemon never writes a review summary,
    status comment, or summary draft. A round with no new finding posts no
    review. Historical summary comments remain untouched and recognisable.
    A comment with no path, no line from 1, no title or no body answers 400,
    and so does a `request_changes` with no P0 or P1 among its comments and
    none of the review's still open: a change request shows its blockers on their
    lines. A comment on a line the change does not show answers 400 too,
    naming the path and the line, before anything reaches the forge: the
    lines are those `get_diff` shows on its new side, read from the same
    diff (`ShownLines`), since a forge takes an inline comment nowhere
    else — GitHub refuses one on a submitted review, which would end the round
    half held. A session with no worktree left has no diff to check
    against, and its comments go on unchecked.
    - GitHub: one `gh api repos/<owner>/<name>/pulls/<n>/reviews` call with
      `REQUEST_CHANGES` or `COMMENT`, the head as `commit_id`, every inline
      comment and no body; a forge whose own answer says the review lacks a
      body takes it again with a body that is the hidden review mark alone,
      `<!-- ariadne:review -->`, so the retry still shows the reader nothing.
      A forge that refuses answers 502 with its own reason — GitHub's
      `{"message", "errors"}` beside the CLI's line — so the session can act
      on it, and the skill says the failure rather than posting another way. The daemon then
      reads the review's comments back.
    The daemon marks the posted comments `from_review`
    (`pull_request_comment_marks`, 026 rule 10), as it marks a review
    session's replies, holds them beside the last read until the next fetch
    reads them back, and answers them with 201. On a request of
    the user's own, or one with a keeper, a `from_review` comment counts as
    another login's: it is told once to the task's author or keeper, as
    "by the Ariadne review", and waits on it until that agent replies (026 rules 12 to 16). No additional
    review-level comment is posted: each finding is already inline. The other way, a
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
    whether the keeping agent believes every required approval and
    check reads green — a task's column agent, or a keeper session on a
    request asked for by hand (032) — together with `head_sha`, the hex sha
    of the head it actually read before confirming readiness. A `ready: true`
    report with no `head_sha`, or one that fails the same hex-sha check as
    `reviewed_sha`, is refused with 400: a readiness claim binds to a
    revision, never floats free of one. `pull_requests.reviewed_sha`,
    `pull_requests.ready` and `pull_requests.ready_head_sha` are written,
    and none raises anything of its own on the session: a pr-reviewer
    session finishing its review must notify nobody, and a `ready` claim
    alone is not confirmed evidence. The `pull_request` attention producer
    (031 rule 13) reads the forge's own evidence against the request's
    current state instead of trusting either call, and withholds the
    readiness item once a later push leaves `ready_head_sha` behind the
    request's current head.
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
    The conversation is no thread, and nothing resolves it.

## Verdict and attention

17. `pr-reviewer` asks for changes when an active P0 or P1 finding exists,
    where the forge permits it, and comments otherwise. P0 is a critical
    risk. P1 is a proven defect, unmet requirement, or written repository-rule
    violation. P2 is optional. What a finding must name to stand at P0 or
    P1, the downgrade to P2 and its cap, the diff as scope, and the verdict
    written first are 017 rule 11's to state, for `pr-reviewer` as for the
    other four review skills. It posts only new inline findings. It creates
    no summary, status table, summary comment, or summary draft. It records
    the reviewed revision and evidence that covers its current head.

    Each new finding has a title, its trigger, a failure and a fix. Where the fix replaces the commented line alone, a ```suggestion
    block under the fix holds the new text of that one line, and a fix
    across lines, across files, or one a test must prove stays prose. On a
    later round it settles each thread it opened before it hunts, reading
    the author's reply: where the defect is actually gone at the head it
    resolves silently; where the last reply claims a fix (`Fixed in <sha>: ...`) and the defect survives, it
    gets one reply, `Still there: <why>.`, never posted twice in the same
    thread; any other answered reply that leaves the defect in place —
    `Not changed: ...` or any other — gets nothing back and stays open for
    the user; a thread nobody has
    answered gets nothing; it waits.

18. A review posted, or a request reported ready, notifies nobody by
    itself: the approval and the merge stay the user's to give in their own
    time, and only the `pull_request` attention producer's own readiness
    item (031), confirmed against the forge's own evidence, tells the user
    a request of theirs is actually ready to merge. A pr-reviewer session's
    own completed review raises no notification, no attention row and no
    badge increment of any kind.

## Acceptance criteria

- A Dependabot request without a review assignment starts a reviewer, keeps
  it after the forge confirms no assignment, loads the picked skills, and
  stops it on explicit stop:
  `pull_request_reviews.rs::an_unassigned_dependabot_request_is_reviewed_until_stopped`.
- A review ask makes one active goal with no orchestrator, on a snapshot of
  `pr-review`, with one task in its `review` column and one agent whose
  session is the review; the goal, the task and the request name each other;
  a repeated ask keeps them:
  `store.rs::a_review_start_makes_one_active_goal_with_one_staffed_task`,
  `::a_repeated_review_start_keeps_the_open_goal_and_moves_its_pin`,
  `pull_request_reviews.rs::a_review_is_one_goal_with_one_staffed_task_and_a_repeat_keeps_it`,
  `::an_open_request_i_review_gets_one_session_detached_at_its_head`.
- A merge completes the goal and its task, a close fails the task and leaves the goal active, a stop cancels both,
  and the next ask makes a new goal:
  `store.rs::ending_a_review_closes_its_task_and_goal_and_the_next_start_is_new`,
  `pull_request_reviews.rs::a_merged_request_completes_its_review_goal_and_task`,
  `::a_closed_request_fails_its_review_task`,
  `::stopping_a_review_cancels_its_goal_and_the_next_ask_starts_a_new_one`.
- A scheduler pass between a start's goal and its ask waits for the start,
  a stop and a start at once leave one whole state, and cancelling the goal
  from the Goals board stops the review for good:
  `pull_request_reviews.rs::a_pass_between_the_goal_and_the_ask_of_a_start_waits_for_the_start`,
  `::a_stop_and_a_start_at_once_leave_one_whole_state`,
  `::cancelling_the_review_goal_stops_the_review_and_it_stays_stopped`,
  `::cancelling_a_replaced_review_goal_leaves_its_replacement_alone`.
- A model outside the `review` column's band is refused and starts no goal:
  `pull_request_reviews.rs::a_review_refuses_a_model_outside_the_workflow_band`.
- Withdrawing a GitHub review request does not cancel a manual review,
  before or after a review was posted:
  `pull_request_reviews.rs::a_withdrawn_review_request_keeps_its_manually_asked_review`,
  `::a_review_posted_after_a_withdrawal_keeps_the_manual_review`,
  `::a_request_withdrawn_after_my_review_keeps_its_manual_review`.

- A review asked on a draft starts at once, and the same session runs on
  as the request leaves draft and goes back to it:
  `pull_request_reviews.rs::an_asked_review_starts_on_a_draft_and_outlives_its_draft_state`.
- An open request gets one live session on the review pin, seat
  `reviewer`, detached at `head_sha`, with `pr-reviewer` indexed:
  `pull_request_reviews.rs::an_open_request_i_review_gets_one_session_detached_at_its_head`.
- GitHub's open list names the login among a request's own `reviewRequests`
  to say the request still asks for the user's review; a team named there,
  by a slug rather than a login, never does:
  `forge/github/pulls.rs::tests::a_user_review_request_names_its_login`,
  `::a_team_review_request_matches_no_login`.
- A detail read with an empty cache preserves a held review request until
  the next fetch checks its timeline. A confirmed withdrawal removes the row:
  `pull_request_reviews.rs::a_detail_read_before_any_fetch_keeps_a_review_request_asking`.
- The review session resolves its own fixed finding through `resolveReviewThread`
  after a verified fix. A thread someone else opened and a resolve
  from another session are refused without a forge mutation:
  `pull_request_reviews.rs::a_review_resolves_its_own_fixed_finding`.
- A round posts only its inline findings, accepts `request_changes` for active
  P0 or P1 findings, and refuses approval. A round with no findings does not
  call the forge:
  `pull_request_reviews.rs::a_review_posts_its_findings_by_priority_and_an_approval_is_refused`.
  The author's answer in a review's thread reaches the review:
  `kept_requests.rs::an_ariadne_review_of_a_kept_request_reaches_its_pr_agent`.
- A review GitHub refuses for having no body is retried with the hidden review
  mark alone, never visible text:
  `github/reviews.rs::tests::a_review_refused_for_its_body_is_retried_with_no_visible_text`.
- A trailer an agent's skill appends to a finding body is stripped before it is
  signed:
  `pull_request_reviews.rs::a_review_strips_a_trailing_trailer_from_its_findings`.
- `get_diff` accepts `since` only when that revision is an ancestor of the
  reviewed head; a present but unrelated revision answers 400:
  `pull_request_reviews.rs::the_diff_reads_the_worktree_against_its_base_and_since_narrows_it`.
- The reviewer briefing fills every placeholder it names:
  `prompts.rs::tests::the_pull_request_texts_fill_every_placeholder_they_name`.
- The reviewer PR seat lists its ten tools and no task or message tool,
  and its tools call the routes of its request:
  `mcp.rs::tests::the_pull_request_reviewer_seat_lists_its_tools_and_no_task_or_message_tool`,
  `tools.rs::tests::the_pull_request_reviewer_tools_call_the_routes_of_the_sessions_request`.

- The skill is within its caps, names the three priorities, permits
  `request_changes` for P0 and P1, never approves or lands a request, and
  reuses verification evidence only for the reviewed head. A `Not changed:`
  reply gets no reply back, while a claimed fix that fails verification gets
  one `Still there: <why>.` reply; a small local fix carries a `suggestion`
  block:
  `skill_tests/review.rs::the_pr_reviewer_skill_ranks_its_findings_and_never_approves`,
  `skill_tests/template.rs::skill_size_caps_hold`.
- A request of mine is no work and gets no review until the user asks; the
  ask starts the work, and the review runs on the pin and with the skills
  they picked, detached at its head, and is a comment; stopping the asking
  ends it and the row, its session let go of it; asked again on another
  model, the review is a fresh session on that model:
  `pull_request_reviews.rs::a_request_of_mine_is_reviewed_once_asked_and_its_review_is_a_comment`.
- Asking needs a model the catalog holds and skills a task agent is staffed
  on, and a refusal starts no work. Asking is taken, not refused, on a
  request that asks for my review, the same as on a request of mine:
  `pull_request_reviews.rs::asking_needs_a_model_and_takes_a_review_request_of_anothers`.
- A review's findings are inline comments titled `[Pn] Title`; a change
  request with no P0 inline and a comment with no title are refused:
  `pull_request_reviews.rs::a_review_posts_its_findings_by_priority_and_an_approval_is_refused`.
- The desktop's request panel starts an Ariadne review of any open request
  on the model and skills picked in its dialog — `pr-reviewer` always on and
  nothing else picked to start with — closes the dialog and opens the new
  review session's console in the panel once the daemon starts it, stops it,
  regardless of its author:
  `pull-request-panel.test.tsx::starts and stops a review of a Dependabot request without a review assignment`.
  A review resumed on its last session opens that console the same way:
  `::opens the console of a review resumed on the same session`. A request
  that asks for the user's review offers the same manual start as a
  request of the user's own, on every repository:
  `::offers a manual Start review on every request that asks for my review`.
- Neither a posted review nor a `ready` report raises a row, a toast or a
  badge of its own: a pull request session's own `waiting_user` carries no
  reason this board, the strip or the sessions screen's filter reads any
  more
  (`attention/board.rs::a_pull_request_sessions_own_waiting_user_raises_no_row_of_its_own_on_this_board`).
- `ariadne pr review` sends the pin it is given, is refused naming `--model`
  with no model and no `--stop`, and `--stop` sends `asked: false` alone and
  conflicts with every asking flag:
  `pr.rs::tests::pr_review_sends_the_ask_body_with_its_pin_and_the_stop_body_alone`,
  `::pr_review_with_no_model_and_no_stop_is_refused`,
  `::pr_review_refuses_stop_combined_with_an_asking_flag_and_a_bare_default_model`.

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/skills/pr-reviewer/SKILL.md`,
`crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-daemon/src/forge/news.rs`, `crates/ariadne-daemon/src/forge/poll.rs`,
`crates/ariadne-daemon/src/forge/github/reviews.rs`,
`crates/ariadne-daemon/src/forge/github/comments.rs`,
`crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-daemon/src/attention/pull_requests.rs`
(031), `crates/ariadne-cli/src/commands/mcp.rs`, `crates/ariadne-cli/src/commands/mcp/tools.rs`,
`crates/ariadne-cli/src/commands/pr.rs`, `ui/src/features/sessions/session-display.tsx`,
`ui/src/features/pull-requests/pull-request-panel.tsx`.
