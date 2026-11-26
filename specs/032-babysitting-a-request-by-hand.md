---
id: babysitting-a-request-by-hand
status: current
updated: 2026-11-26
areas: [store, api, daemon, cli, mcp, prompts, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/pull_request_babysit.rs
  - crates/ariadne-daemon/src/scheduler/pull_requests.rs
  - crates/ariadne-daemon/src/forge/poll.rs
  - crates/ariadne-daemon/src/launcher.rs
  - crates/ariadne-daemon/src/http/pull_requests.rs
  - crates/ariadne-daemon/src/attention/pull_requests.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/it/workflows.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-core/src/lib.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/pr.rs
  - crates/ariadne-daemon/tests/it/attention.rs
  - ui/src/features/pull-requests/pull-request-panel.test.tsx
  - ui/src/features/pull-requests/pull-requests-page.test.tsx
---

# Babysitting a request by hand

An open request that no task opened gets a keeper session once
the user asks Ariadne to keep it: a session on a seat of its own, in a
worktree on a branch of its own, that commits a fix and pushes it as the
briefing says.

## Scope

In: which requests can be asked for, the ask route, the goal and task it
makes, the seat, the worktree and its branch, the briefing, the news, the
tools, and the attention this raises.

Out: the request a task opens and the agent of its `pr` column that keeps it
([030](030-workflows.md)); the reviewer seat and the review it posts
([029](029-reviewing-a-request.md)); the rows Ariadne keeps and the forge
reads behind them ([026](026-pull-requests.md)).

## Behavior

1. Any open request that no task keeps can be asked for, regardless of
   whether its author is the user, another person or a bot. A request a
   task already keeps (`origin_task_id`) is refused with 409 to avoid
   competing keepers. Git push permissions still apply to the head repository.
2. The session sits on the request alone, on a new seat, `keeper`. It is the
   session of the one agent of a babysit operation's task (rule 7b), so it
   carries that goal, task and agent, the same as a reviewer session does
   (029 rule 10b). A keeper from before such goals has none: its
   `agent_sessions.goal_id`, `task_id` and `task_agent_id` are NULL.
3. The worktree is `worktree_root/pr-<id>-keep`, on a local branch
   `ariadne/keep-<id>`, cut from the request's head: the head branch is
   fetched from the repository's remote (or the head repository of a fork,
   as a reviewer's detached worktree is), and the branch is cut at that
   fetched head, or at the commit the checkout already holds where it does.
   Unlike a reviewer's worktree, a later push never moves it: the keeper
   commits there, so only the keeper moves its own branch.
4. The push target is `<remote> HEAD:<head_branch>`: the keeper pushes its
   local branch onto the request's own head branch, on the repository's
   remote. The keeper runs this line in its own shell, so the remote and
   the head branch are each quoted into one argument: git's own ref rules
   let a branch name hold `$`, `(`, `;`, `|`, `&` or a backtick, none of
   which the quoting leaves the shell to read as its own (026 rule 26).
5. The briefing names the request, the worktree, its local branch, the
   base, the login and the push command. The title, the URL and the base
   branch name are fenced, the same as a reviewer's own (026 rule 26);
   the worktree path, the login and the local branch — Ariadne's own
   name, `ariadne/keep-<id>` — are not. The task and goal of a keeper
   session are the daemon's own and carry the request's title, so 006 rule
   19's plain-text treatment of a task's title never applies here: the
   title, the URL and the base branch name stay the forge's own, fenced
   regardless. A keeper session refuses to start on
   a head branch git itself refuses as a ref, the same refusal a
   reviewer session's own spawn makes (026 rule 26). Its recovery mark
   is separate from a reviewer mark on the same
   request, and its recovery clears only that keeper mark, whether a new
   session starts or its keeper reports again. The seat text
   says what it owes: keep one open request the
   user asked it to keep, work only in the worktree on its branch, commit a fix and
   push it as the briefing says, and that Ariadne wakes it with the news
   and ends it on the merge or the close, then to end its turn.
6. The session loads what its agent was staffed with: `pr-babysit` — the
   same skill the `pr` column of a task stages — through the fixed
   `babysit` column (030 rule 11), and the skills the user picked when
   asking, each one a task agent is staffed on. A keeper from before
   goals loads `pr-babysit` and the skills stored on its request row. Its
   comment batching, branch-update and review-scope rules are 030 rule 10.

## Asking on a request, by hand

7. `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-babysit` takes
   `{asked, model, effort, skills}`, the shape of `AskReviewRequest` (029).
   `model` is required to ask and is checked against the catalog as any pin
   is; a missing or unknown one answers 400. `model` must also have a rank
   the fixed `babysit` column allows (030 rule 11): an unranked model, or
   one of another rank, answers 400 naming the band — the band of the open
   goal's snapshot where one is open, else of the fixed column. Each skill must be one a task agent is staffed
   on, else 400; `pr-babysit` is loaded anyway and is not stored. Asking
   an open request starts Ariadne's work on it, makes the babysit goal
   (rule 7b), writes `pull_requests.babysit_asked`, the pin `babysit_model` and
   `babysit_effort`, and `babysit_skills`; a request that is not open
   answers 409. Asking a second time moves the pin and the skills onto the
   new ask. `asked: false` clears the pin and the skills, cancels the
   babysit goal and takes the keeper session down.

7b. A babysit ask makes one active goal of the babysit operation, and a
    stop or an end finishes it. It is the lifecycle 029 rule 10b states for
    every PR operation (`Store::start_pr_operation`,
    `Store::end_pr_operation`), applied to the keeper:
    - The goal runs on `pr-babysit`, snapshotted into `goal_steps`, is
      `active` at once and has no orchestrator. It has one task, already
      `in_progress` on the `babysit` column, and one agent: the pin of the
      ask and the column's skills with the picked ones. The request's own
      pass starts, resumes and watches the keeper session (rule 9), and the
      session carries the goal, the task and the agent, so the Goals board,
      the task panel, the session panel, its history and its usage read it
      as any task's. Nothing about the keeper's worktree, branch, push
      target, permissions or news changes.
    - A repeated ask keeps the open goal, its task and its agent and moves
      the pin and skills onto the agent; the keeper restarts on the new pin
      as rule 9 says, on the same session row where it can.
    - A merge completes the task and the goal; a close fails the task and leaves the goal active; a stop, a goal
      whose ask is gone, or an integration turned off cancels both. The
      next ask after a stop makes a new goal. A session belongs to the goal
      it started under: a keeper from before goals is not adopted at
      rollout and runs on as it did, with no goal, until the user asks
      again, and that ask makes the goal and puts a new session under it on
      the same worktree and branch, so nothing unpushed is lost.
    - A request a task keeps is refused (rule 1), and the goal of one
      operation never blocks the other: a babysit and a review of the same
      request are two goals on two tasks, and stopping one leaves the
      other. At most one babysit goal is open per request; a repeat reuses
      it.
    - The start, the stop and the part of a scheduler pass that acts on
      the goal hold the one lock of the request (029 rule 10b), so two
      asks at once make one goal and one task, and the goal and the ask
      always stand together, across a daemon restart too: the goal names
      the request by value, and the pass finds it again.
    - Cancelling the goal from the Goals board is a stop (029 rule 10b).
8. `ariadne pr babysit <repo> <number> --model AGENT:MODEL [--effort E]
   [--skill S]... [--attach]` and `ariadne pr babysit <repo> <number>
   --stop` call this route from the CLI, mirroring `pr review` (014):
   `--model` is required with no `--stop`, and a usage error names the flag
   when it is missing. `--attach` waits for the keeper session this starts
   and opens its console.

## The scheduler

9. A row with `babysit_asked` wants one live keeper session while
   the request is open: started or resumed on the pin asked directly on its
   own row, in the worktree above. A session that went away on its own is
   resumed on its own row and conversation, unless the pin moved since it
   started, the same as a reviewer session is (029 rule 5a).
10. The keeper receives the same immediate comment and new-head news as a task agent (026 rule 12).
    Its head mark is independent of the reviewer mark.
11. Babysitting ends on an explicit stop, merge or close: the babysit goal
    and its task end (rule 7b), the keeper session is killed, its worktree and its local branch are removed, and Ariadne
    stops working on the request, unless a review is still asked of it.
    Asking again on a later open request starts a fresh keeper session.
    An ask that lands while a pass is stopping work on a request
    nobody asked anything of is not lost: the row goes only as the pass
    read it (026 rule 19), so the ask stays and the pass it wakes starts
    the keeper session.

## Tools and reports

12. The keeper seat lists `get_pull_request`, `get_diff`, `list_comments`,
    `get_comment`, `reply_comment`, `escalate_comment`,
    beside
    `wait_for_user` and `withdraw_wait_for_user` — the same six request
    tools the agent of a task's `pr` column has, and no task tool and no
    message tool: the user reaches the session through its console, as a
    reviewer's. No tool resolves a thread and none submits a review: those
    stay the reviewer seat's alone (029). `escalate_comment` hands a
    disputed thread to the user (026 rule 15): the escalate route accepts
    the keeper of this row (`Seat::Keeper`), posts nothing on the forge,
    and the `pull_request` producer raises one item for the thread until a
    human settles it (031 rule 17).
13. `reply_comment` from the keeper follows the keeping agent's own rule
    (026 rule 16): it answers an inline review comment, the thread of the
    defect, and a comment of the other side in the conversation, once. It
    may not answer a comment of its own side, nor one its own side already
    answered there. The daemon accepts the keeper seat for this the same
    way it accepts a task's column agent: a request a task keeps is
    refused the ask (rule 1), so no row ever carries both, and the seat
    alone tells the two apart.
14. No keeper reports readiness. The daemon decides from forge evidence (031 rule 15).
15. The readiness item covers a hand-kept request with the same evidence as a task-kept request (031 rule 15).
## Desktop

16. The Pull requests panel offers Start babysit and Stop babysit in the
    Babysit group, beside the Review group, on any open request that no task keeps.
    The dialog asks the model, the effort and the skills beside
    `pr-babysit`, which is always loaded and cannot be turned off. Starting
    closes the dialog and, once the daemon has the keeper session up, opens
    its console in the panel, the same way starting a review does (029); an
    "Ariadne babysitting" pill marks the row and the panel header while one
    is asked, beside "Ariadne reviewing", and links the babysit's goal as
    **Babysit goal**. The panel's Sessions tab lists the
    keeper session beside a review's, since both carry the request's id.

17. The keeper follows the batch, reply and dispute rules of 030 rule 10.
    It reports no readiness, marks no draft, posts no status comment and leaves every thread open.
    It asks the user once about failures the request did not cause and fixes none.
    It ends its turn on a merge or close.
    The daemon promotes drafts kept by hand under 026 rule 27. No seat has a draft-mark tool.
    `POST /v1/repositories/{id}/pull-requests/{number}/keeper-turn` accepts an empty body and returns 202.
    It wakes a task's keeper or a keeper by hand with untold news immediately.
    With no news, it asks the keeper to read the request again. No keeper returns 409.
    `pr babysit <repo> <number> --again` calls it, including on task-kept requests.
    `--again` conflicts with `--model`, `--effort`, `--skill`, `--attach` and `--stop`.
    The panel shows a **Babysit** button group on an open request: **Start babysit** when no keeper runs, **Stop babysit** with **New turn** while a babysitter runs, and only **New turn** on a task-kept request. **New turn** calls the keeper-turn route.
## Acceptance criteria

- A request of Ariadne's maps to the author role; Dependabot's and another
  person's map to reviewer. A manual ask for each starts a keeper that
  receives comments and failing-check news, raises readiness attention once
  backed by forge evidence, and cleans up on merge:
  `forge/pulls.rs::tests::an_integrations_author_is_its_author_and_everyone_else_is_a_reviewer`,
  `pull_request_babysit.rs::asking_keeps_a_request_of_mine_by_hand_until_the_merge`,
  `::asking_keeps_a_dependabot_request_until_the_merge`,
  `::asking_keeps_another_authors_request_until_the_merge`.
- Review and babysitting can coexist on a Dependabot request; stopping
  either preserves the other, and stopping both removes the request row:
  `pull_request_babysit.rs::stopping_review_keeps_a_dependabot_keeper`,
  `pull_request_babysit.rs::stopping_babysitting_keeps_a_dependabot_reviewer`.

- Concurrent review and babysit retain independent head delivery:
  `pull_request_babysit.rs::keeper_push_and_external_push_reach_the_reviewer_independently`.

- Asking on a request starts one keeper session, seat `keeper`,
  with `pr-babysit` indexed, its worktree on `ariadne/keep-<id>`; a comment
  by another login reaches it once; daemon readiness uses forge evidence; and the
  merge ends it, with its worktree and its branch gone and the row gone:
  `pull_request_babysit.rs::asking_keeps_a_dependabot_request_until_the_merge`.
- A babysit ask makes one active goal with no orchestrator, on a snapshot of
  `pr-babysit`, with one task in its `babysit` column and one agent whose
  session is the keeper; a repeated ask keeps them, and two asks at once
  make one goal:
  `store.rs::a_babysit_start_makes_one_active_goal_with_one_staffed_task`,
  `pull_request_babysit.rs::a_babysit_is_one_goal_with_one_staffed_task_and_a_repeat_keeps_it`,
  `::two_babysit_asks_at_once_make_one_goal`.
- A merge completes the goal and its task, a close fails the task and leaves the goal active, a stop cancels both,
  and the next ask makes a new goal:
  `store.rs::a_repeated_babysit_start_keeps_its_goal_and_an_end_leaves_the_next_start_new`,
  `pull_request_babysit.rs::a_merged_request_completes_its_babysit_goal_and_task`,
  `::a_closed_request_fails_its_babysit_task`,
  `::stopping_a_babysit_cancels_its_goal_and_the_next_ask_starts_a_new_one`.
- Cancelling the goal from the Goals board stops the babysit for good:
  `pull_request_babysit.rs::cancelling_the_babysit_goal_stops_the_babysit_and_it_stays_stopped`.
- A model outside the `babysit` column's band is refused and starts no goal:
  `pull_request_babysit.rs::a_babysit_refuses_a_model_outside_the_workflow_band`.
- A keeper from before goals is not adopted: it runs on until an ask puts a
  session under a new goal, on the same worktree:
  `pull_request_babysit.rs::a_keeper_from_before_goals_runs_on_until_an_ask_puts_it_under_a_goal`.
- A babysit and a review of one request are two goals, and stopping one
  leaves the other:
  `pull_request_babysit.rs::a_babysit_and_a_review_of_one_request_are_two_goals`.
- A babysit runs on its fixed `babysit` column and on no workflow of the
  catalog, and no ordinary goal or repository default names `pr-babysit`:
  `defaults.rs::each_pr_operation_runs_one_fixed_column_and_no_shipped_workflow`,
  `store.rs::pull_request_operations_run_on_no_workflow_of_the_catalog`,
  `tests/it/workflows.rs::the_catalog_holds_no_pull_request_operation_workflow`.
- Asking is refused with no model, an unknown one, a skill no task agent
  loads, a request that is not open, or one a task
  already keeps — each starting no work:
  `pull_request_babysit.rs::asking_is_refused_with_no_model_an_unknown_one_an_untaskable_skill_or_the_wrong_request`.
- Asking writes the pin and the skills; asking again moves them; `asked:
  false` clears every one of them:
  `store.rs::babysitting_a_request_of_mine_writes_its_pin_and_its_skills`.
- `pr babysit` sends the ask body with its pin and the stop body alone, and
  is refused naming `--model` with no model and no `--stop`:
  `pr.rs::tests::pr_babysit_sends_the_ask_body_with_its_pin_and_the_stop_body_alone`,
  `::pr_babysit_with_no_model_and_no_stop_is_refused`,
  `::pr_babysit_refuses_stop_combined_with_an_asking_flag_and_a_bare_default_model`.
- `pr ls` names babysitting beside a review in its `ariadne` column, and a
  task's own keeping wins over both:
  `pr.rs::tests::pr_ls_names_babysitting_beside_a_review_in_the_ariadne_column`.
  `pr inspect` prints the ask:
  `pr.rs::tests::pr_inspect_prints_readable_keys_in_order_with_the_body_last`.
- The keeper seat lists its six request tools, `escalate_comment`
  among them, and no task or message tool:
  `mcp.rs::tests::the_pull_request_keeper_seat_lists_its_tools_and_no_task_or_message_tool`.
- The keeper escalates a thread of the row it keeps, and the thread's
  item appears (rule 12):
  `attention.rs::each_escalated_thread_raises_its_own_item_until_the_forge_settles_it`.
- The keeper briefing fills every placeholder it names, its title, URL
  and base branch name fenced the same as a reviewer's own, since a
  keeper session carries no task or goal for 006 rule 19's plain-text
  treatment to reach:
  `prompts.rs::tests::the_keeper_briefing_fills_every_placeholder_it_names`.
- The push command quotes the remote and the head branch into one
  argument each, a real shell reads either back as the exact value
  whatever it holds, and a branch with shell metacharacters stays one
  argument of the line a request is actually kept on:
  `launcher.rs::shell_quote_tests::a_shell_reads_a_quoted_value_back_as_one_argument`,
  `::a_push_command_keeps_a_metacharacter_laden_branch_as_one_argument`,
  `pull_request_babysit.rs::asking_keeps_a_dependabot_request_until_the_merge`,
  `::a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository`,
  `::a_keeper_on_the_same_repository_pushes_through_the_configured_remote`.
- A keeper head-branch refusal and recovery use the keeper's recovery mark
  alone, and a keeper recovery cannot clear a reviewer mark on the same row:
  `scheduler_attention.rs::a_keeper_session_refuses_to_start_on_a_head_branch_git_refuses_as_a_ref`,
  `::a_keeper_recovers_once_its_head_branch_turns_valid_again`,
  `::a_keeper_that_reports_after_giving_up_clears_its_recovery`,
  `::a_keeper_recovery_keeps_a_reviewers_give_up_mark`.
- The four seats answer to the word the wire spells each with:
  `lib.rs::tests::the_seats_are_the_orchestrator_the_agent_the_pull_request_reviewer_and_the_keeper`.
- The panel starts a keeper session on the model and skills picked, opens
  its console, stops it, and offers the buttons on any open request that
  no task keeps:
  `pull-request-panel.test.tsx::starts and stops babysitting a Dependabot request without a review assignment`,
  `::offers Start babysit on a request of mine that no task keeps`,
  `::offers Start babysit on a request that asks for my review`,
  `::offers no Start babysit on a request of mine a task already keeps`.
- The panel links the babysit's goal to the Goals board:
  `pull-request-panel.test.tsx::links the goal of a babysit to the Goals board`.
- A row shows the Ariadne babysitting pill while a keeper session babysits
  it by hand:
  `pull-requests-page.test.tsx::shows the Ariadne babysitting pill on a row a keeper session babysits by hand`.

- A draft kept by hand is marked ready:
  `pull_request_babysit.rs::a_draft_kept_by_hand_is_marked_ready`.
- The daemon promotes only its own checked draft, once per head:
  `kept_requests.rs::the_daemon_promotes_only_its_own_checked_draft_once_per_head`.
- Keeper turn wakes a task keeper and refuses an unkept request:
  `kept_requests.rs::a_keeper_turn_wakes_a_task_kept_request_and_refuses_an_unkept_request`.
- The CLI sends an empty keeper-turn request:
  `pr.rs::tests::pr_babysit_again_posts_an_empty_keeper_turn`.
- The CLI refuses incompatible keeper-turn flags:
  `pr.rs::tests::pr_babysit_again_conflicts_with_every_start_or_stop_flag`.
- The panel sends the Babysit New turn on a task-kept request and shows a refusal:
  `pull-request-panel.test.tsx::asks for a keeper turn on a task kept request and shows the daemon refusal`.
- The skill names its two seats once, and a keeper fixes only what the
  request's own change breaks:
  `skill_tests/operate.rs::the_pr_babysit_skill_names_its_two_seats_once`,
  `::the_pr_babysit_skill_fixes_only_what_the_requests_change_breaks`.

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/migrations/0003_keeper_recovery.sql`,
`crates/ariadne-store/src/pull_requests.rs`, `crates/ariadne-store/src/pr_operations.rs`,
`crates/ariadne-store/src/defaults.rs`,
`crates/ariadne-core/src/lib.rs`,
`crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-daemon/src/forge/poll.rs`, `crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/agents/prompts.rs`,
`crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/attention/pull_requests.rs`,
`crates/ariadne-cli/src/commands/mcp.rs`, `crates/ariadne-cli/src/commands/pr.rs`,
`ui/src/features/pull-requests/`.
