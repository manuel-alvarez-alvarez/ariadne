---
id: reviewing-a-request
status: current
updated: 2026-10-08
areas: [store, api, daemon, cli, mcp, prompts, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/pull_request_reviews.rs
  - crates/ariadne-daemon/tests/it/kept_requests.rs
  - crates/ariadne-daemon/src/forge/news.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - ui/src/features/goals/attention.test.tsx
  - ui/src/features/pull-requests/pull-requests-page.test.tsx
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

Out: the ledger, the fetch, the comments table and the PR session kind
([026](026-pull-requests.md)); the skill catalog and staffing
([017](017-skills-and-staffed-agents.md)); the tool surface as a whole
([013](013-mcp-tool-surface.md)).

## Behavior

1. A row with role `reviewer` wants one live session when it is `open`, not a
   draft, still asks for the user's review, and its repository's integration
   is enabled and names a `review_model`. The daemon staffs it on
   `review_model` and `review_effort` (025), with seat `reviewer` and the
   `pr-reviewer` skill (017). A repository with no `review_model` starts
   nothing, and its reviewer rows get no detail fetch. A row with role
   `author` wants one where `review_asked` holds: the user asked Ariadne to
   review a request of their own (rule 10). That session runs on the pin the
   user picked, `pull_requests.review_model` and `review_effort`, and loads
   `pr-reviewer` and the skills they picked, `review_skills`; the
   repository's `review_model` has no part in it.
2. A draft starts nothing. When the request leaves draft, the next fetch
   starts the session. A request that goes back to draft ends it.
3. `pull_requests.review_requested` records whether the request still asks
   for the user's review. The fetch's list of review requests that holds it
   says yes (026 rule 5); a row the list of open requests alone holds, and
   that never asked, says no and asks the forge nothing. For an open
   forge-tracked row that asked before and the list no longer holds, the
   fetch asks the forge
   (`ForgeClient::review_still_requested`). GitHub stops listing a request
   once the user reviewed it, and writes no removal event for that. So it
   reads the request's timeline: the last review request or removed request
   for the login decides. A review decides nothing, so a review posted after a
   withdrawal does not ask again. GitLab keeps a reviewer listed after a review,
   so a request it does not list asks for nothing. A review of the user's own
   keeps nothing alive. Where the forge cannot answer, the flag stays as it
   was. A request tracked by hand keeps the flag it has.
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
   the threads the integration login opened, each told once (026 rule 18).
   A push is a `head_sha` that differs from `told_head_sha`. Its line names
   the new head and the last `reviewed_sha`, and asks for a review of the
   commits since it. Checks, the base, the review decision and the state are
   no news to a reviewer. On a request of the user's own a push alone is
   news: its comments are the news of the task's author that keeps it (026),
   and each is told once.
8. A push waits for the session to be idle. Then the daemon moves the
   worktree to the new head (`checkout_detached`) and hands the news in one
   prompt. A fetch with the same head hands nothing.
9. A row that turns `merged` or `closed`, goes back to draft, or loses the
   user's review request has its sessions killed and its worktree removed. No
   branch is touched. An ended row records `cleaned_at` (026 rule 23); an
   ended request of the user's own leaves that record to the cleanup of its
   author's side, and so does a request of theirs whose asking stopped.

## Asking on a request of the user's own

10. `PUT /v1/pull-requests/{id}/ariadne-review` takes `{asked, model,
    effort, skills}` and writes `pull_requests.review_asked`, the pin
    `review_model` and `review_effort`, and `review_skills` (migrations
    `0014` and `0016`). `model` is required to ask and is checked against the
    catalog as any pin is; a missing or unknown one answers 400. Each skill
    must be one a task agent is staffed on (017), else 400; `pr-reviewer` is
    loaded anyway and is not stored. It answers the row, and wakes the fetch
    and the scheduler. A row with role `reviewer` is refused with 409: it
    has a review session of its own already, on the repository's pin.
    Asking on an ended request is refused with 409. `asked: false` clears
    the pin and the skills and takes the session down.

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
    or `P2`). `body` is a summary of the findings tied to no line; each
    finding is one inline comment on the line of its defect. The daemon
    posts each as `**[P0] Title**`, a blank line, then its body: what goes
    wrong and how to fix it. A comment with no path, no line from 1, no title
    or no body answers 400, and so does a `request_changes` with no P0
    finding among its comments: a change request shows its P0s on their
    lines.
    - GitHub: one `gh api repos/<owner>/<name>/pulls/<n>/reviews` call with
      `REQUEST_CHANGES` or `COMMENT`, the head as `commit_id`, and every
      inline comment. The daemon then reads the review's comments back.
    - GitLab: one discussion per inline comment on the merge request's diff,
      through `glab api`, then one summary note. GitLab has no review verdict,
      so the note starts with "Request changes" where the review asks for
      changes. Each position names the file on both sides of the diff: the
      merge request's `diffs` are read before anything is posted, and a
      renamed file takes its old path as `old_path`.
    The daemon stores the posted body and comments as comments of the
    integration login, marked `from_review` (migration `0018`), as it marks
    a review session's replies, and answers them with 201. On a request of
    the user's own a `from_review` comment counts as another login's: it is
    told once to the task's author, as "by the Ariadne review", and waits
    on it until the author replies (026 rules 17 to 19). On a request of the user's
    own the review is a comment whatever its `event`: no forge takes a
    change request from a request's own author.
14. Every `event` but `request_changes` and `comment` answers 400 before the
    forge sees a call. An approval is refused above all: the user gives it.
    No route approves or merges.
15. `report_pull_request` takes `reviewed_sha`, a hex sha, on a reviewer row,
    or on a row of the user's own with `review_asked`, alone. It writes `pull_requests.reviewed_sha`. A sha that moves it raises
    `waiting_user` on the session again; the same sha raises nothing.
16. Diffs, reviews and reports are accepted from the request's own session
    alone. Another session, or a call with no session, gets 403.
16a. `POST /v1/pull-requests/{id}/comments/{comment_id}/resolve` resolves
    the thread of that comment on the forge and marks every stored comment
    of it resolved, then counts the threads that wait again. Only the
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
    is worth fixing. It writes the review body as a short verdict on the
    findings — the count of each priority and each finding's title — with
    no file, no line and nothing of what it did, and each finding as one
    inline comment with a title, the failure and a fix. On a later round it
    replies on each earlier finding, and resolves the thread of each one a
    push fixed.
18. `waiting_user` on a reviewer PR session reads "review posted, approve
    yourself" in `ariadne attention` and "Review posted, approve yourself" in
    the desktop app. The approval and the merge stay the user's.

## Acceptance criteria

- An open request out of draft gets one live session on the review pin, seat
  `reviewer`, detached at `head_sha`, with `pr-reviewer` indexed:
  `pull_request_reviews.rs::an_open_request_i_review_gets_one_session_detached_at_its_head`.
- The review session resolves the thread of its own fixed finding through
  `resolveReviewThread`, and a thread someone else opened, or a resolve from
  another session, is refused:
  `pull_request_reviews.rs::a_review_resolves_the_thread_of_its_own_fixed_finding_and_no_other`.
- On GitLab a finding on a renamed file names its old path:
  `pull_request_reviews.rs::a_gitlab_finding_on_a_renamed_file_names_its_old_path`.
- An Ariadne review of a request a task's author keeps reaches that author,
  waits on it, and is answered by its reply:
  `kept_requests.rs::an_ariadne_review_of_a_kept_request_reaches_its_author`.
- A draft gets none until it leaves draft:
  `pull_request_reviews.rs::a_draft_starts_no_review_until_it_leaves_draft`.
- A repository with no `review_model` gets none:
  `pull_request_reviews.rs::a_repository_with_no_review_model_starts_no_review`.
- A push moves the worktree and hands one prompt that names the last
  `reviewed_sha`; the same head again hands nothing:
  `pull_request_reviews.rs::a_push_moves_the_worktree_and_is_told_once_with_the_last_reviewed_sha`,
  `news.rs::tests::a_push_to_a_request_i_review_is_told_once_with_the_last_reviewed_sha`.
- `get_diff` answers the diff against the base, and `since` narrows it:
  `pull_request_reviews.rs::the_diff_reads_the_worktree_against_its_base_and_since_narrows_it`.
- A review with `request_changes` and two comments runs the forge's review
  call with `REQUEST_CHANGES`, both comments and their priorities, and stores
  them; `approve` answers 400 and the forge sees no call:
  `pull_request_reviews.rs::a_review_posts_its_findings_by_priority_and_an_approval_is_refused`.
- A `reviewed_sha` is stored and raises `waiting_user`; a later sha raises it
  again:
  `pull_request_reviews.rs::a_reviewed_sha_raises_waiting_user_and_a_later_one_raises_it_again`,
  `store.rs::a_reviewed_sha_moves_once_and_a_reviewer_hears_of_its_own_threads`.
- A merged, closed or withdrawn request ends the session and removes the
  worktree:
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
- The reviewer PR seat lists its seven tools and no task or message tool, and
  its tools call the routes of its request:
  `mcp.rs::tests::the_pull_request_reviewer_seat_lists_its_seven_tools_and_no_task_or_message_tool`,
  `tools.rs::tests::the_pull_request_reviewer_tools_call_the_routes_of_the_sessions_request`.
- The skill is within its caps, names the three priorities, names
  `request_changes` only beside P0, resolves a thread a push fixed, and
  names no approve, merge, sleep, poll, `gh` or `glab`:
  `defaults.rs::tests::the_pr_reviewer_skill_ranks_its_findings_and_never_approves`,
  `::skill_size_caps_hold`.
- A request of mine gets no review until the user asks, then one on the pin
  and with the skills they picked, detached at its head, whose review is a
  comment, and stopping the asking ends it; asked again on another model,
  the review is a fresh session on that model:
  `pull_request_reviews.rs::a_request_of_mine_is_reviewed_once_asked_and_its_review_is_a_comment`.
- Asking needs a model the catalog holds and skills a task agent is staffed
  on, and is refused on a request that asks for my review:
  `pull_request_reviews.rs::asking_needs_a_model_and_a_request_of_mine`.
- A review's findings are inline comments titled `[Pn] Title`; a change
  request with no P0 inline and a comment with no title are refused:
  `pull_request_reviews.rs::a_review_posts_its_findings_by_priority_and_an_approval_is_refused`.
- The desktop's request panel starts an Ariadne review of a request of mine
  on the model and skills picked in its dialog — `pr-reviewer` always on and
  nothing else picked to start with — closes the dialog and opens the new
  review session's console in the panel once the daemon starts it, stops it,
  and offers it on no other:
  `pull-request-panel.test.tsx::starts an Ariadne review of a request of mine on the model and skills picked, opens its console, and stops it`,
  `::offers no Ariadne review on a request that asks for my review`. A review
  resumed on its last session opens that console the same way:
  `::opens the console of a review resumed on the same session`.
- The attention text says the review is posted and the approval is the
  user's:
  `attention.rs::tests::a_session_is_reported_for_the_reason_the_ui_would_give`,
  `attention.test.tsx::carries a reviewer pull request session's waiting_user as review posted`.

## Sources

`crates/ariadne-store/migrations/0011_pull_request_reviews.sql`,
`crates/ariadne-store/migrations/0014_ariadne_review_of_my_requests.sql`,
`crates/ariadne-store/skills/pr-reviewer/SKILL.md`,
`crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-daemon/src/forge/news.rs`, `crates/ariadne-daemon/src/forge/poll.rs`,
`crates/ariadne-daemon/src/forge/github/reviews.rs`,
`crates/ariadne-daemon/src/forge/gitlab/reviews.rs`,
`crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-cli/src/commands/mcp.rs`, `crates/ariadne-cli/src/commands/mcp/tools.rs`.
