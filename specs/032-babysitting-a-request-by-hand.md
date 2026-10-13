---
id: babysitting-a-request-by-hand
status: current
updated: 2026-10-13
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
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-core/src/lib.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/pr.rs
  - ui/src/features/pull-requests/pull-request-panel.test.tsx
  - ui/src/features/pull-requests/pull-requests-page.test.tsx
---

# Babysitting a request by hand

A request of the user's own that no task opened gets a keeper session once
the user asks Ariadne to keep it: a session on a seat of its own, in a
worktree on a branch of its own, that commits a fix and pushes it as the
briefing says.

## Scope

In: which requests can be asked for, the ask route, the seat, the worktree
and its branch, the briefing, the news, the tools, and the attention this
raises.

Out: the request a task opens and the agent of its `pr` column that keeps it
([030](030-workflows.md)); the reviewer seat and the review it posts
([029](029-reviewing-a-request.md)); the rows Ariadne keeps and the forge
reads behind them ([026](026-pull-requests.md)).

## Behavior

1. Only a request of the user's own that no task opened can be asked for.
   Asking on a request by another login, or on one a task already keeps
   (`origin_task_id`), is refused with 409.
2. The session sits on the request alone, on a new seat, `keeper`. It has no
   goal and no task: `agent_sessions.goal_id` and `task_id` are both NULL,
   the same as a reviewer session's.
3. The worktree is `worktree_root/pr-<id>-keep`, on a local branch
   `ariadne/keep-<id>`, cut from the request's head: the head branch is
   fetched from the repository's remote (or the head repository of a fork,
   as a reviewer's detached worktree is), and the branch is cut at that
   fetched head, or at the commit the checkout already holds where it does.
   Unlike a reviewer's worktree, a later push never moves it: the keeper
   commits there, so only the keeper moves its own branch.
4. The push target is `<remote> HEAD:<head_branch>`: the keeper pushes its
   local branch onto the request's own head branch, on the repository's
   remote.
5. The briefing names the request, the worktree, its local branch, the
   base, the login and the push command. The seat text says what it owes:
   keep one open request of the user's own, work only in the worktree on
   its branch, commit a fix and push it as the briefing says, and that
   Ariadne wakes it with the news and ends it on the merge or the close,
   then to end its turn.
6. The session loads `pr-babysit` — the same skill the `pr` column of a
   task stages (030) — and the skills the user picked when asking, each one
   a task agent is staffed on.

## Asking on a request, by hand

7. `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-babysit` takes
   `{asked, model, effort, skills}`, the shape of `AskReviewRequest` (029).
   `model` is required to ask and is checked against the catalog as any pin
   is; a missing or unknown one answers 400. Each skill must be one a task
   agent is staffed on, else 400; `pr-babysit` is loaded anyway and is not
   stored. Asking an open request starts Ariadne's work on it, writes
   `pull_requests.babysit_asked`, the pin `babysit_model` and
   `babysit_effort`, and `babysit_skills`; a request that is not open
   answers 409. Asking a second time moves the pin and the skills onto the
   new ask. `asked: false` clears the pin and the skills and takes the
   keeper session down.
8. `ariadne pr babysit <repo> <number> --model AGENT:MODEL [--effort E]
   [--skill S]... [--attach]` and `ariadne pr babysit <repo> <number>
   --stop` call this route from the CLI, mirroring `pr review` (014):
   `--model` is required with no `--stop`, and a usage error names the flag
   when it is missing. `--attach` waits for the keeper session this starts
   and opens its console.

## The scheduler

9. A row of mine with `babysit_asked` wants one live keeper session while
   the request is open: started or resumed on the pin asked directly on its
   own row, in the worktree above. A session that went away on its own is
   resumed on its own row and conversation, unless the pin moved since it
   started, the same as a reviewer session is (029 rule 5a).
10. The author's news (026) reaches the keeper session exactly as it
    reaches the agent of a task's `pr` column: comments by another login,
    checks that turn to failure, the rolled-up check state, the review
    decision and the state, each told once.
11. Unlike a self-review — which only ends on an explicit `asked: false`
    — babysitting also ends on the merge or the close: the keeper session
    is killed, its worktree and its local branch are removed, and Ariadne
    stops working on the request, unless a review is still asked of it.
    Asking again on a later open request starts a fresh keeper session.
    An ask that lands while a pass is stopping work on a request of mine
    nobody asked anything of is not lost: the row goes only as the pass
    read it (026 rule 19), so the ask stays and the pass it wakes starts
    the keeper session.

## Tools and reports

12. The keeper seat lists `get_pull_request`, `get_diff`, `list_comments`,
    `get_comment`, `reply_comment` and `report_pull_request`, beside
    `wait_for_user` and `withdraw_wait_for_user` — the same six request
    tools the agent of a task's `pr` column has, and no task tool and no
    message tool: the user reaches the session through its console, as a
    reviewer's. No tool resolves a thread and none submits a review: those
    stay the reviewer seat's alone (029).
13. `reply_comment` from the keeper follows the keeping agent's own rule
    (026 rule 16): it answers an inline review comment, the thread of the
    defect, and a comment of the other side in the conversation, once. It
    may not answer a comment of its own side, nor one its own side already
    answered there. The daemon accepts the keeper seat for this the same
    way it accepts a task's column agent: a request a task keeps is
    refused the ask (rule 1), so no row ever carries both, and the seat
    alone tells the two apart.
    A repository that supervises its posts holds the keeper's reply back
    the same way (026 rule 24): nothing posts to the forge directly, the
    route answers `pending: true`, and the comment counts answered from
    then on, so a later poll hands the keeper no further news of it.
14. `report_pull_request` takes `ready` with `head_sha`, on a row this
    keeper answers for, by the same rule a task's column agent reports
    under (029 rule 15): a `ready: true` report with no `head_sha`, or one
    that fails the hex-sha check, is refused with 400. On a row
    `babysit_asked` keeps, only the keeper session reports it ready — never
    a reviewer session the same row also carries, where a self-review was
    asked of it too.
15. The `pull_request` attention producer's readiness item (031) covers a
    row kept by hand the same way it covers a task-kept one: the same forge
    evidence, confirmed against the head the keeper's report named.

## Desktop

16. The Pull requests panel offers Start babysit and Stop babysit beside
    Start review and Stop review, on a request of the user's own that no
    task keeps. The dialog asks the model, the effort and the skills beside
    `pr-babysit`, which is always loaded and cannot be turned off. Starting
    closes the dialog and, once the daemon has the keeper session up, opens
    its console in the panel, the same way starting a review does (029); an
    "Ariadne babysitting" pill marks the row and the panel header while one
    is asked, beside "Ariadne reviewing". The panel's Sessions tab lists the
    keeper session beside a review's, since both carry the request's id.

## Acceptance criteria

- Asking on a request of mine starts one keeper session, seat `keeper`,
  with `pr-babysit` indexed, its worktree on `ariadne/keep-<id>`; a comment
  by another login reaches it once; its `ready` report is accepted; and the
  merge ends it, with its worktree and its branch gone and the row gone:
  `pull_request_babysit.rs::asking_keeps_a_request_of_mine_by_hand_until_the_merge`.
- Where the repository supervises its posts, the keeper's reply to a
  conversation comment answers `pending: true` with the pending review's
  own id, posts no `gh pr comment`, leaves the comment answered with no
  further reply taken, is refused a second reply to it, and a later poll
  hands the keeper no further news of it:
  `pull_request_babysit.rs::a_keeper_holds_its_reply_pending_when_the_repository_supervises_posts`.
- Asking is refused with no model, an unknown one, a skill no task agent
  loads, a request that is not open, one by another login, or one a task
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
- The keeper seat lists its six tools and no task or message tool:
  `mcp.rs::tests::the_pull_request_keeper_seat_lists_its_tools_and_no_task_or_message_tool`.
- The keeper briefing fills every placeholder it names:
  `prompts.rs::tests::the_keeper_briefing_fills_every_placeholder_it_names`.
- The four seats answer to the word the wire spells each with:
  `lib.rs::tests::the_seats_are_the_orchestrator_the_agent_the_pull_request_reviewer_and_the_keeper`.
- The panel starts a keeper session on the model and skills picked, opens
  its console, stops it, and offers the buttons on a request of mine that
  no task keeps alone:
  `pull-request-panel.test.tsx::starts Ariadne babysitting of a request of mine on the model and skills picked, opens its console, and stops it`,
  `::offers Start babysit on a request of mine that no task keeps, and on no other request`,
  `::offers no Start babysit on a request that asks for my review`,
  `::offers no Start babysit on a request of mine a task already keeps`.
- A row shows the Ariadne babysitting pill while a keeper session babysits
  it by hand:
  `pull-requests-page.test.tsx::shows the Ariadne babysitting pill on a row a keeper session babysits by hand`.

## Sources

`crates/ariadne-store/migrations/0005_pull_request_babysitting.sql`,
`crates/ariadne-store/src/pull_requests.rs`, `crates/ariadne-store/src/defaults.rs`,
`crates/ariadne-core/src/lib.rs`,
`crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-daemon/src/forge/poll.rs`, `crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/agents/prompts.rs`,
`crates/ariadne-daemon/src/http/pull_requests.rs`,
`crates/ariadne-daemon/src/attention/pull_requests.rs`,
`crates/ariadne-cli/src/commands/mcp.rs`, `crates/ariadne-cli/src/commands/pr.rs`,
`ui/src/features/pull-requests/`.
