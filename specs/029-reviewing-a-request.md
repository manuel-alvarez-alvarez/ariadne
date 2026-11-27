---
id: reviewing-a-request
status: current
updated: 2026-11-27
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
  - crates/ariadne-store/tests/pr_operation_task_migration.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/tests/attention.rs
  - crates/ariadne-cli/src/commands/pr.rs
  - crates/ariadne-store/src/skill_tests/review.rs
  - ui/src/features/pull-requests/pull-requests-page.test.tsx
  - ui/src/features/pull-requests/pull-request-panel.test.tsx
---

# Reviewing a request

Any open request, regardless of its author or assigned reviewers,
gets a review session once the user asks Ariadne for one — never on its
own. The ask makes one staffed task with no goal, and the review session is
that task's agent (rule 10b). The daemon starts rounds as rules 7 and 8 specify. It posts one review in the user's name, and the forge approval stays
the user's. Ariadne holds no approval of its own for a request: the status
rule 15a once gave is retired (rule 15a).

## Scope

In: which rows want a review session, the manual ask on any request, the
task it makes, its worktree, its news, its tools and routes, the
verdict policy, the retired Ariadne approval, and the attention it raises.

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
   `review_skills`, on the review's task (rule 10b). A GitHub review request still creates a row on the
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
   states: "You review one open pull or merge request that the user explicitly asks Ariadne to review.
   The author can be the user, another person or a bot. The user needs no GitHub review assignment.
   The request can be a draft. Work only in your worktree, detached at the head of the request.
   Commit nothing and push nothing."
5a. A review session that went away is resumed on its own row and
   conversation, unless the pin moved since it started: a conversation keeps
   the model it started on, so a new model or effort is a fresh session on
   it.
6. `pull_requests.told_head_sha` is the head the session was last told of.
   Each start and resume writes it to `head_sha`, since the briefing names
   that head.
7. A reviewer receives only round prompts. Replies from the other side in threads with a `from_review` comment wake it.
   Other comments, checks and review decisions never wake it. Each round reads its threads with `list_comments`.
   The initial briefing asks for a round on the current head.
   Each later `PULL_REQUEST_NEWS` contains one line starting `- Review again:`.
   It names the reason: new head, thread reply, forge re-request or user ask.
   The head and last reviewed sha are each fenced as data; an absent reviewed sha reads `none`.
8. A changed head starts a round immediately on both `author` and `reviewer` requests.
   No review push cool-down applies.
   A reply from the other side after a `from_review` comment in the same thread also starts a round.
   The conversation and other threads start no round.
   Delivery marks each reply in `reviewer_told_at`, separately from the keeper’s `told_at`.
   Each seat receives a human reply once; one seat’s delivery never consumes the other seat’s news.
   Proof: `kept_requests.rs::an_ariadne_review_of_a_kept_request_reaches_its_pr_agent`.
   A reviewer-role request that the forge's open list names again after session start, and did not name on the last read, queues a forge re-request round. It does so even where `review_requested` stayed true after a completed review.
   The fetch queues the round only after it keeps the read of that request.
   The round then names the head of that read and runs on it.
   An assignment already present at session start queues nothing more.
   A user ask through `POST /v1/repositories/{id}/pull-requests/{number}/review-round`
   queues a round immediately. The route takes an empty body and answers 202.
   Without `review_asked`, it answers 409. OpenAPI includes the route.
   Each queued round waits until the session is idle.
   The daemon moves the detached worktree to the current head before sending its prompt.
   Multiple pending asks can share one round.
   Proof: `pull_request_reviews.rs::a_push_moves_the_worktree_and_is_told_once_with_the_last_reviewed_sha`,
   `::a_push_reaches_an_idle_session_without_a_wait`,
   `::a_review_round_needs_an_ask_and_runs_on_the_current_head`,
   `::a_reply_in_a_review_thread_starts_one_round`,
   `::a_forge_re_request_round_waits_for_the_read_of_the_new_head`.
9. A request that turns merged or closed, or whose manual ask is stopped,
   has its reviewer session killed and its detached worktree removed. A
   merge finishes the review's task; a close fails it; a stop cancels it
   (rule 10b).
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
    stored. `model` must have a rank the fixed `review` column allows (030
    rule 11): an unranked model, or one of another rank, answers 400 naming
    the band — the band of the open task's snapshot where one is open, else
    of the fixed column. It answers the request,
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
    from the CLI (014), on any request: `--model` is required with neither
    `--stop` nor `--again`,
    since there is no repository pin to fall back to any more, and names
    the flag when it is missing. `--attach` waits for the review session
    this starts and opens its console.

10aa. `pr review <repo> <number> --again` calls the round route in rule 8.
    It conflicts with `--model`, `--effort`, `--skill`, `--attach` and `--stop`.
    The request panel offers **New turn** beside **Stop review** in the **Review** group while review is asked.
    It shows the daemon's refusal text.

10b. A review ask makes one task of the review operation, with no goal,
    and a stop or an end finishes it. The lifecycle is shared by every PR
    operation (`Store::start_pr_operation`, `Store::end_pr_operation`); the
    babysit operation applies the same one (032 rule 7b).
    - The task names its request by value in `task_pull_requests`
      (`operation`, `repository_id`, `number`, `url`): the request's own row
      goes when Ariadne stops working on it, and the task stays as history.
      One task per request and operation is the open one (`open`), kept by a
      partial unique index. A finished or cancelled task stops being the
      open one; a failed one stays it until a stop or the next ask.
    - The task belongs to no goal. Its one column is the fixed `review`
      column (030 rule 11), snapshotted into `remote_task_steps` as a
      goal-less task's columns are, and it records `pr-review` as its
      workflow name in `task_workflows`. It is `in_progress` on that column
      at once, with one agent: the pin of the ask and the column's skills
      with the picked ones. No orchestrator plans it, and no task pass runs
      it: the request's own pass starts, resumes and watches its session
      (rule 1), and the session carries the task and the agent, so the
      board, the task panel, the session panel, its history and its usage
      read it as any goal-less task's.
    - A repeated ask keeps the open task and its agent, and moves the pin and
      skills onto the agent. The live session runs on, and the next start
      uses the new pin.
    - A merge finishes the task; a close fails it; a stop, a task whose ask
      is gone, or an integration turned off cancels it. The next ask after a
      stop or a close makes a new task and a new session: a session belongs
      to the task it started under, and one left over from another task, or
      from before operation tasks, is ended and not adopted.
    - One lock per request (`Store::pr_operation_lock`) serializes a start,
      a stop and the part of a scheduler pass that acts on the task. The
      pass rereads the row under the lock, so it never reads a start whose
      task is made and whose ask is not, nor a stop half done. A start
      writes the task and then the ask; a stop clears the ask and then
      cancels the task (`Store::stop_pr_operation`), so no pass starts a
      session for an ask nobody owns. A pass still ends a task whose ask is
      gone, that was ended by hand, or whose request ended, so a failed
      write converges. Starting twice at once makes one task.
    - Cancelling the task (`POST /v1/tasks/{id}/cancel`) is a stop: it
      clears the ask and cancels the task under the same lock, so Forge
      shows the review stopped and no session starts afterwards. The task
      is read again under the lock: one a stop and a new start replaced
      while the cancel waited answers 409, and the replacement stays. The
      stop is the user's alone: an orchestrator or a column, review or
      keeper session that cancels an operation's task answers 403, and the
      task, its ask and its session stay.
    - `TaskDto.operation` and `PullRequestDto.operations` name the operation
      and the open task of each; a review task's `pr_url` names its request.
      The desktop's request panel links the task, and `pr show` prints it.
    - The upgrade to this shape (migration 0029) turns each review or
      babysit goal from before into its task: the task keeps its column,
      workflow name, sessions and history, loses its goal, and is the open
      one where the goal was still active; the goals and the `goals.pr_*`
      columns go.

## Tools and routes

11. The reviewer PR seat lists `get_pull_request`, `get_diff`,
    `list_comments`, `get_comment`, `reply_comment`, `escalate_comment`,
    `resolve_thread` and `submit_review`, and no task
    tool and no message tool (013). The user reaches the session through its
    console (008). `escalate_comment` takes a `comment_id` of a comment the
    review posted alone: a `from_review` comment of the integration login.
    The route refuses any other comment with 403 (026 rule 15). The thread
    reaches Needs attention as the keeping agent's escalation does (031
    rule 17):
    `kept_requests.rs::the_review_session_escalates_only_a_comment_the_review_posted`,
    `attention.rs::a_review_sessions_escalation_raises_the_same_item_as_a_keepers`,
    `mcp.rs::the_pull_request_reviewer_seat_lists_its_tools_and_no_task_or_message_tool`.
12. `GET /v1/pull-requests/{id}/diff?since=` answers `git diff <base>...HEAD`
    in the session's worktree, or `git diff <since>..HEAD` only where `since`
    is an ancestor of the current head. The base is `<remote>/<base_branch>`
    where the checkout holds it, else the local base branch. A `since` that is
    not a hex sha of a commit in the worktree, or is not an ancestor, answers
    400 so rewritten history receives the full diff.
13. `POST /v1/pull-requests/{id}/reviews` takes `event`, a round `body`, and
    `comments` of `path`, `line`, `title`, `body` and `priority` (`P0`, `P1`
    or `P2`). One call is one round of the review. `comments` are the
    round's new findings, each one inline comment on the line of its
    defect, posted as `**[P0] Title**`, a blank line, then its body: what
    goes wrong and how to fix it, with a trailer an agent's skill appends
    stripped first (026 rule 16). The daemon posts `body` above the round's inline findings with the review marker.
    The marker preserves its identity when the forge returns it.
    Each round writes its own body, including rounds with no new findings.
    The body has three parts, in order. Its first line is `Reviewed <sha>.`
    Then one short prose paragraph describes the defects, with no bullet,
    no table, no line per finding and no line per check. One closing
    sentence states whether the author must change the code, such as
    `Changes required: fix the P1 before this lands.` or `No change required.`
    The reviewer skill writes this body before calling `submit_review`, and
    submits every round with `event` `comment`. The route still takes both
    events.
    Historical summary comments remain untouched and recognisable.
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
      comment and the round body; an older caller that leaves the body empty
      takes a retry when the forge says it lacks a body.
      The retry uses the hidden review mark alone,
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
    "by the Ariadne review", and waits on it until that agent replies (026 rules 12 to 16). No separate
    status comment is posted: each finding is already inline. The other way, a
    comment under the login that no review posted — the author's answer,
    or the user's — in a thread a review opened is the review session's
    news, so the answer to a finding reaches its reviewer. On a request of the user's
    own the review is a comment whatever its `event`: no forge takes a
    change request from a request's own author.
14. Every `event` but `request_changes` and `comment` answers 400 before the
    forge sees a call. A forge approval is refused above all: the user gives
    it. No route approves on the forge or merges.
15. `submit_review` writes `pull_requests.reviewed_sha` from the head of the checkout the round reviewed,
    and posts the review on that head, not on a live forge head fetched since
    (`pull_request_reviews.rs::a_submitted_review_stores_the_checkout_head_when_the_forge_head_moved`).
    No session reports readiness. The daemon decides from forge evidence (031 rule 15).
    A finding the `pr-reviewer` skill writes follows these rules:
    - The skill gives the title in the `title` field of `submit_review` only.
    - The daemon posts `**[Pn] Title**` from it (rule 13), so the body repeats no title.
    - The body opens with a concise description and has `To reproduce:` steps where needed.
    - A reproduction is a code block of at most 10 lines, or prose steps.
    - A longer reproduction becomes prose steps.
    - A comment names no local file: no file under `/tmp`, no worktree path and no scratch file.
    - Proof code still goes under `/tmp` for the reviewer's own run.
15a. Ariadne holds no approval of its own for a request. The user gives approval.
    No tool approves or merges.
16. Diffs and reviews are accepted from the request's own session
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

17. `pr-reviewer` posts every round as a `comment`, and never names
    `request_changes`: the closing sentence of the round body says whether
    the author must change the code. P0 is a critical risk to behavior,
    data or security. P1 is a proven defect, unmet requirement, or written
    repository-rule violation. P2 is an optional improvement. A finding
    keeps the priority of its severity: a missing proof makes it no P2.
    What a finding must name to stand at P0 or P1, the downgrade of a
    finding with no trigger and the cap on optional findings, the diff as
    scope, and the verdict written first are 017 rule 11's to state, for
    `pr-reviewer` as for the other review skills. It posts only new inline findings. It creates
    no summary, status table, summary comment, or summary draft. It records
    the reviewed revision and evidence that covers its current head. It
    never gives a forge approval, and Ariadne holds no approval of its own
    (rule 15a).

    A round reads the ask, diff, checks, owned threads, hunt, self-disproof pass, then posts.
    The ask is the request body's purpose and linked issue or ticket, read before the diff.
    Claims in descriptions, commits and replies prove nothing.
    A check passes only in this round at the reviewed sha.
    Run changed files' tests, lint and format checks; forge checks stand for the rest.
    A failed forge check is a P1 only after the diff is shown to cause it.
    Hunt behavior, errors, races, boundaries, security, data loss, tests and required behavior.
    Check deleted, skipped or weakened tests, removed negative cases, weak assertions and tests that mock the defective seam.
    A regression test must fail when the defect returns.
    An unrequested change needs a concrete consequence to become a finding.
    Try reproduction at the reviewed sha, with scratch code under `/tmp`.
    Quote conflicting lines for text defects.
    Refute each candidate with a code citation before posting; drop refuted candidates.
    Failed reproduction keeps severity and states `Unverified:` with its cause.

    Each finding is one inline comment on a shown new-side line.
    The title, body and reproduction rules of a finding are in rule 15.
    Include the trigger, rule, expected behavior, observed failure, evidence and needed change.
    Use the shown causal line for defects elsewhere, naming the real file and line.
    Report unrelated defects only in the turn result.
    Use a suggestion block for a one-line fix.
    The body opens `Reviewed <short sha>.`.
    Add at most two sentences on the change and main risk, then one sentence per new finding.
    Close with `Changes required: <what>.` while a must-fix finding remains open, else `No change required.`
    Write no check line or finding list; post the body even with no new findings.

    Read owned threads each round. Verify `Fixed in <short sha>:` at the head and resolve silently when fixed.
    Otherwise reply once `Still there: <what the head shows>.`.
    Await push news for fixes outside the worktree revision.
    Resolve `Not changed:` on a P2.
    On the first P0 or P1 rejection, reply once with a current-head reproduction or withdraw and resolve.
    On the second rejection, call `escalate_comment` on the first comment and post nothing more.
    Give a one-sentence reason; escalate again only after a new dispute.
    Plain disagreement without proof also counts as rejection.
    Check disproof, reply `Withdrawn: <why>.`, and resolve.
    Answer each question once in one sentence. Give thanks and unanswered threads no reply.
    Leave threads another login opened.
    Escalated threads reach Needs attention (031 rule 17).

    A draft gets the same round. News of a head equal to the reviewed
    revision needs no tool call. Where the head moves during a round, the
    daemon records the revision the round reviewed. The round ends on one result
    in the turn text, in a literal shape: `Reviewed <sha>: comment with <n>
    findings.`, with `0` for a round with no new finding, then one line per
    finding, ending `unverified` where its reproduction failed, settled
    thread, check, failed tool call and defect outside the diff. That text
    is all the user reads of a round that posts nothing. The round ends on
    `submit_review` alone: the skill states that
    a step call another skill names — `complete_step` or `fail_step` in a
    picked `code-review` or `documentation` — is the task seat's, and that
    this seat has none, since the seat lists neither tool (rule 11).

18. A review posted notifies nobody by
    itself: the approval and the merge stay the user's to give in their own
    time, and only the `pull_request` attention producer's own readiness
    item (031), confirmed against the forge's own evidence, tells the user
    a request of theirs is actually ready to merge. A pr-reviewer session's
    own completed review raises no notification, no attention row and no
    badge increment of any kind.

## Acceptance criteria

- The review session escalates a thread its own review opened: a
  `from_review` comment of the integration login gets 200, posts nothing on
  the forge and marks the comment. Any other comment gets 403 and no mark
  (rule 11):
  `kept_requests.rs::the_review_session_escalates_only_a_comment_the_review_posted`.
- After one evidence exchange, the `pr-reviewer` skill escalates a disputed thread and posts nothing
  in it, and its `Thread` line accepts `escalated` (rule 17):
  `skill_tests/review.rs::the_pr_reviewer_skill_ranks_its_findings_and_never_approves`.
- The reviewer seat lists `escalate_comment` beside its request tools, and
  no other seat gains it (rule 11):
  `mcp.rs::the_pull_request_reviewer_seat_lists_its_tools_and_no_task_or_message_tool`,
  `::every_seat_has_the_tools_its_playbook_names_and_no_others`.
- The tool calls the escalate route of the session's request (rule 11):
  `tools.rs::the_pull_request_reviewer_tools_call_the_routes_of_the_sessions_request`.
- A review session's escalation raises the same item as a keeper's
  (rule 17, 031 rule 17):
  `attention.rs::a_review_sessions_escalation_raises_the_same_item_as_a_keepers`.
- A Dependabot request without a review assignment starts a reviewer, keeps
  it after the forge confirms no assignment, loads the picked skills, and
  stops it on explicit stop:
  `pull_request_reviews.rs::an_unassigned_dependabot_request_is_reviewed_until_stopped`.
- A review ask makes one task with no goal and no orchestrator, in the
  fixed `review` column, with one agent whose session is the review; the
  task and the request name each other; a repeated ask keeps them:
  `store.rs::a_review_start_makes_one_goal_less_task_in_its_column`,
  `::a_repeated_review_start_keeps_the_open_task_and_moves_its_pin`,
  `pull_request_reviews.rs::a_review_is_one_goal_less_task_and_a_repeat_keeps_it`,
  `::an_open_request_i_review_gets_one_session_detached_at_its_head`.
- A merge finishes the task, a close fails it and it stays the open one, a
  stop cancels it, and the next ask makes a new task:
  `store.rs::ending_a_review_closes_its_task_and_the_next_start_is_new`,
  `::a_closed_request_leaves_its_failed_task_open_until_the_next_start`,
  `pull_request_reviews.rs::a_merged_request_finishes_its_review_task`,
  `::a_closed_request_fails_its_review_task`,
  `::stopping_a_review_cancels_its_task_and_the_next_ask_starts_a_new_one`.
- A scheduler pass between a start's task and its ask waits for the start,
  a stop and a start at once leave one whole state, cancelling the task
  stops the review for good, and the review's own session cannot cancel it:
  `pull_request_reviews.rs::a_pass_between_the_task_and_the_ask_of_a_start_waits_for_the_start`,
  `::a_stop_and_a_start_at_once_leave_one_whole_state`,
  `::cancelling_the_review_task_stops_the_review_and_it_stays_stopped`,
  `::cancelling_a_replaced_review_task_leaves_its_replacement_alone`,
  `::the_review_session_cannot_cancel_its_own_task`.
- The upgrade turns each review and babysit goal into its goal-less task and
  leaves ordinary goals as they were:
  `pr_operation_task_migration.rs::the_upgrade_turns_operation_goals_into_goal_less_tasks`.
- A model outside the `review` column's band is refused and starts no task:
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
- The reviewer skill writes, for every round, a body of the reviewed sha, one prose paragraph on the defects and a closing sentence on whether the code must change:
  `skill_tests/review.rs::the_pr_reviewer_skill_ranks_its_findings_and_never_approves`.
- A round posts its submitted body and inline findings, accepts `request_changes` for active
  P0 or P1 findings, and refuses approval. A round with no findings posts its own body:
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
- The reviewer PR seat lists its eleven tools and no task or message tool,
  and its tools call the routes of its request:
  `mcp.rs::tests::the_pull_request_reviewer_seat_lists_its_tools_and_no_task_or_message_tool`,
  `tools.rs::tests::the_pull_request_reviewer_tools_call_the_routes_of_the_sessions_request`.

- The skill is within its caps, names the three priorities, submits
  every round as a `comment` and never names `request_changes`, names no approval, never
  gives a forge approval or lands a request, and
  reuses its evidence only for the reviewed revision. It runs the checks
  of the changed files before it settles its threads, and settles them
  before it hunts. It tries to reproduce each finding, keeps its
  priority where the reproduction fails and opens `Proof:` with
  `Unverified:`, and the body has four labelled paragraphs. A finding the diff causes on a
  line it does not show sits on the shown line that causes it. A `Not changed:`
  reply gets no reply back, while a claimed fix that fails verification gets
  one `Still there: <why>.` reply, a question gets one answer and a finding
  proven wrong is withdrawn; a small local fix carries a `suggestion`
  block. The round ends on the literal result shape:
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
- A round body does not answer an earlier conversation comment:
  `forge::live::tests::a_round_body_does_not_answer_a_conversation_comment`.
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
- No posted review raises a row, a toast or a
  badge of its own: a pull request session's own `waiting_user` carries no
  reason this board, the strip or the sessions screen's filter reads any
  more
  (`crates/ariadne-cli/tests/attention.rs::incomplete_json_contains_only_daemon_items_and_their_count`).
- `ariadne pr review` sends the pin it is given, is refused naming `--model`
  with no model and no `--stop`, and `--stop` sends `asked: false` alone and
  conflicts with every asking flag:
  `pr.rs::tests::pr_review_sends_the_ask_body_with_its_pin_and_the_stop_body_alone`,
  `::pr_review_with_no_model_and_no_stop_is_refused`,
  `::pr_review_refuses_stop_combined_with_an_asking_flag_and_a_bare_default_model`.

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/migrations/0009_drop_pull_request_approvals.sql`,
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
