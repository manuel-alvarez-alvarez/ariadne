---
id: how-a-task-ends
status: current
updated: 2026-10-07
areas: [daemon, store, prompts]
commits: [ad268ee0, 305ee064, 45c5e131, 8174c256, 90ac6e67, 524856c7, fdd0c5b6, a69b953f, 29e6d84e, f79c8e15, a4d7da95]
tests:
  - crates/ariadne-daemon/tests/it/landing_lifecycle.rs
  - crates/ariadne-daemon/tests/it/final_tasks.rs
  - crates/ariadne-daemon/tests/it/scheduler_attention.rs
  - crates/ariadne-daemon/tests/it/skill_documents.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-daemon/tests/it/repositories.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
---

# How a task ends

Four endings, one of which lands nothing, chosen once for the whole goal and
run by the agent that did the work.

## Scope

In: the endings and where each is decided, the procedure of each, merge
verification, and opening the request a `pull_request` ending lands by.

Out: who approves the change (004), the state machine around `approved` and
`finished` (001), the forge a repository's remote is on and whether Ariadne
works with it (025), and what becomes of a request once it is open (026).

## Behavior

1. A task ends the way its goal's `landing` names. The user chooses it once,
   when the goal is created, and every task of the goal follows it:
   - `merge` — the author puts the change on the base branch itself;
   - `pull_request` — the author opens a request and pushes the branch it was
     opened from; the task ends there, and what happens to the request from
     then on — its comments, its checks, its merge — is not this task's to
     wait on (026);
   - `none` — nothing is landed, and what the task produced is the whole of
     it: a published tag, a filed report, a document that lives elsewhere;
   - `feature_branch` — the tasks land on a branch of the goal, one per
     repository, and the final task of each repository takes that branch
     onto the base branch by one request. A task lands on the goal branch as
     `merge` lands on the base branch: the same briefing and the same
     verification, against the goal branch. Until that branch exists, the
     base branch takes its place.
   All of them reach `finished` (001). Landing is one way of getting there
   rather than the meaning of being there.
2. A goal created with no landing gets the default landing of its first
   repository. A repository defaults to `merge`, and editing its default
   changes no existing goal. The landing cannot change after the goal is
   created. A task has no landing of its own: the task
   requests and the task tools take none, and `TaskDto.landing` is a
   read-only copy of the goal's.
3. The procedure is Ariadne's, one text per ending, and nothing overrides it.
   A repository supplies a default only while a goal is created (002). Each
   text is rendered with `{task_title}`, `{branch}`, `{base_branch}` and
   `{repo_path}`, and may name nothing else.
4. An approved task is landed by its own author, in the session and worktree
   it already holds. There is no separate integrator seat. On a task staffed
   with several authors that author is the picked winner (004), and its
   branch is the one every landing command and check reads. The landing
   briefing is counted as sent only once it has gone out: an approval that
   lands while the author's agent is still coming up — launched, and heard
   from not yet — is briefed once a later pass can reach it.
5. `merge`: rebase the task branch onto the base. When the rebase changes
   nothing and HEAD already equals the reviewer's approved SHA, skip the
   checks. Otherwise, run the whole suite, the build and the linters once.
   Then squash the branch into one commit with a
   Conventional Commits subject, fast-forward the base branch in the primary
   checkout, push where there is a remote, then `finish_task` with the base
   branch's sha. The suite runs after the rebase and before the fast-forward,
   and it is the one run of it the author owes (004): before the rebase it
   proves a tree the base branch never grows, and after the fast-forward a
   failure is a revert rather than a fix. A check that fails is fixed on the
   task branch and rebased again. The push comes before `finish_task`, because
   that call ends the task and the cleanup behind it takes the worktree.
   The squash goes onto the merge base of the task branch and the base
   branch, which is the commit the rebase used. A worktree shares its refs,
   so the base branch can move while the suite runs; a squash onto its name
   would then take back what landed in between. Before the fast-forward,
   `git diff --stat {base_branch} HEAD` must list only files of the task.
   When it lists more, or the fast-forward fails, the base moved, and the
   author starts again from the fetch. On that later pass the whole suite runs
   again only after a conflict, or when a new base commit changes a file of
   the task. Otherwise the checks of the crates that either side changed run
   on the rebased tree: each side's own tree was proven whole by its own
   landing, so the tree that lands is still proven.
6. `pull_request`: rebase once — the only rebase — push the branch, and call
   `open_pull_request` with a title by the repository's own commit
   conventions and a body from its request template. The daemon opens the
   request itself, through `gh` (github.com) or `glab` (GitLab), whichever
   the repository's detected forge names (025): the author never runs either
   CLI. The call answers the URL it opened, stores it on the task, and a
   second call on the same task answers that URL again rather than opening
   another. The task then ends: `finish_task`, with the sha of the branch it
   just pushed.
7. A branch a request was opened from only grows from there: no amend, no
   rebase, no forced push, whether before or after `open_pull_request` is
   called. A base that has moved is merged in and pushed plainly, never
   rebased onto.
8. The author does not wait on the request once it is open: it never polls
   the forge, never answers a comment on it, and never merges it — none of
   that is this task's to do (026). A revision asked for before
   `finish_task` goes back through the Ariadne reviewers like any other
   round (004), pushes the branch again, and reports the same request, which
   `open_pull_request` answers with its existing URL rather than opening a
   second one.
9. One request per task. `open_pull_request` opens it once; a later call on
   the same task — a retry, a resumed author, a revision's second pass —
   answers the URL already recorded rather than opening another. Ariadne
   never merges the request, and nothing here waits for a human to: the task
   finishes once the request is open and its branch is on the remote, not
   once somebody has acted on it.
10. `feature_branch` ends in a final task per repository: the one task that
    depends directly on every other task of that repository. Two such tasks
    would depend on each other, so at most one matches. It needs no reviewer.
    `finalize_plan` refuses a `feature_branch` plan unless every repository
    of the goal has a final task, and the error names the repository. A task
    created while the goal is active joins the `depends_on` of the final task
    of its repository; once that final task has started, the create is
    refused. So the final task stays `pending` until every other task of its
    repository is finished. Then its author works on the goal branch itself:
    its worktree checks out the goal branch, it cuts no task branch, and the
    task's `branch` reads the goal branch from then on. That claim and a task
    create are serialized: a create that comes first sends a `ready` final
    task back to `pending` and its author does not start, and a create that
    comes after is refused before the task exists. That return to `pending`
    is a wait rather than a failed spawn, so no number of them fails the
    final task. It lands on the repository base branch, by the same
    `open_pull_request` procedure as an ordinary task's `pull_request` ending,
    in a landing briefing of its own that names the goal branch and the base
    branch and skips the rebase every other task already did. The task ends
    once that request is open and pushed, the same as any other; the goal
    branch itself is deleted, local and remote, only once the request merges
    — a pull request entity's own doing (026), not this task's.
11. `none`: the author is briefed to check that what the task asked for is
    where the task said to put it, and that nothing is left only in the
    worktree, which is thrown away with the task. Then `finish_task`, with no
    merge commit.
12. `finish_task` is verified against how the task ends. `merge` and
    `feature_branch` accept a sha only once git proves it, with
    `git merge-base --is-ancestor`, an ancestor of the landing branch: a
    merge that never happened is refused. `pull_request` accepts it once the
    task has a `pr_url` and the branch it was opened from is still what the
    remote has for it — no sha is asked onto the base branch, since the
    daemon never merges the request itself. The final task of a
    `feature_branch` goal is verified the same way, once it works on the
    goal branch. A task that lands nothing has nothing git can be asked
    about, so nothing is verified and no sha is demanded.
13. The forge is detected off the repository's remote and stored on it, not
    read fresh at landing time (025): a `forge_integrations` row, enabled or
    not, is what `open_pull_request` needs to find one, and the host it
    names is what the CLI's authentication is checked against. The daemon
    runs the forge's own CLI, `gh` or `glab`, itself — never the agent.

## Acceptance criteria

- An approval that lands while the author's agent is still coming up still
  briefs it to land, once a later pass can reach it, and the agent receives
  the briefing
  (`landing_lifecycle.rs::an_approval_during_the_authors_start_still_briefs_it_to_land`).
- An approved task is landed by its own author
  (`landing_lifecycle.rs::an_approved_task_is_landed_by_its_own_author`), with
  the procedure of the ending its goal carries
  (`prompts.rs::the_task_says_what_the_author_lands_with`,
  `store.rs::a_task_lands_by_the_ending_its_goal_carries`).
- A goal created with `pull_request` briefs every one of its tasks to land by
  pull request, and each task reads that landing back
  (`landing_lifecycle.rs::a_pull_request_goal_briefs_every_task_to_land_by_pull_request`).
- A goal created with no landing takes its first repository's default,
  `TaskDto.landing` is the goal's, and a task create or edit that names a
  landing is refused
  (`repositories.rs::a_repository_defaults_new_goals_without_changing_existing_ones`,
  `landing_lifecycle.rs::a_goal_with_no_landing_merges_and_a_task_takes_none_of_its_own`);
  `create_task` and `update_task` take no landing
  (`tools.rs::the_task_tools_take_no_landing`).
- A `feature_branch` goal lands its tasks like `merge`: the same briefing, and
  a merge that never happened is refused
  (`landing_lifecycle.rs::a_feature_branch_goal_lands_its_tasks_like_merge`).
- A `feature_branch` plan with no final task in a repository is refused, and
  the error names the repository
  (`final_tasks.rs::a_feature_branch_plan_with_no_final_task_is_refused`).
- A task created after the plan is finalized joins the final task's
  `depends_on`
  (`final_tasks.rs::a_task_created_after_finalize_joins_the_final_task`).
- A task created just before the final task starts delays the start, and one
  created after it is refused
  (`final_tasks.rs::a_task_created_before_the_final_task_starts_delays_the_start`,
  `::a_task_created_after_the_final_task_starts_is_refused`). Sent back
  to wait four times in a row, the final task still starts once every task
  it waits for is finished
  (`::a_final_task_sent_back_to_wait_again_and_again_still_starts`).
- The final task stays `pending` until every other task of its repository is
  finished, then starts on the goal branch and cuts no branch. It opens a
  request for the goal branch onto the base branch and finishes the same way
  an ordinary `pull_request` task does, with the goal branch left standing
  (`final_tasks.rs::the_final_task_waits_then_lands_the_goal_branch_on_the_base`).
  The briefing runs its steps in order, names `open_pull_request` and nothing
  after `finish_task`
  (`defaults.rs::the_final_landing_takes_the_goal_branch_onto_the_base_in_order`,
  `::nothing_the_author_still_has_to_run_comes_after_the_call_that_ends_the_task`).
- A merge that never happened is refused
  (`landing_lifecycle.rs::a_merge_that_never_happened_is_refused`).
- `open_pull_request` on an approved, pushed task of a repository with a
  detected forge runs the stub's `pr create` with the head, the base, the
  title and the body, stores the URL, and answers it; a second call answers
  the same URL and the stub sees no second create. An unpushed branch is
  refused with 409 that says to push first
  (`landing_lifecycle.rs::opening_a_pull_request_runs_the_forge_cli_once_and_ends_the_task`).
  A repository with no detected forge is refused with 409
  (`::opening_a_pull_request_refuses_with_no_forge_detected`), and a stub
  whose `auth status` exits 1 is refused with 409 and the probe's message
  (`::opening_a_pull_request_refuses_an_unauthenticated_cli`).
- A `pull_request` task whose request is open and whose branch is on the
  remote finishes on `finish_task` with the branch's sha, with no sha asked
  onto the base branch
  (`landing_lifecycle.rs::opening_a_pull_request_runs_the_forge_cli_once_and_ends_the_task`).
- No `ready` exists any more, and nothing about an open request raises
  attention on its own — a resumed author carries none forward
  (`scheduler_attention.rs::an_open_pull_request_raises_no_attention`).
- A repository supplies the landing for a new goal without one, and editing
  the default changes no existing goal
  (`repositories.rs::a_repository_defaults_new_goals_without_changing_existing_ones`).
- Each ending's briefing is one procedure and nothing of the other
  (`defaults.rs::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`),
  and nothing the author still has to run comes after `finish_task`
  (`defaults.rs::nothing_the_author_still_has_to_run_comes_after_the_call_that_ends_the_task`).
- No landing briefing names a forge merge command, or `gh` or `glab` at all:
  the daemon runs the forge CLI, never the author
  (`defaults.rs::no_landing_names_a_forge_merge_command`).
- The `merge` briefing runs the whole suite once, between the rebase and the
  fast-forward
  (`defaults.rs::the_direct_landing_runs_the_whole_suite_after_the_rebase_and_before_the_fast_forward`),
  and a later pass reruns it only where the base and the task meet (the
  same test).
- A squash that runs after another task landed keeps that landing's work, or
  its fast-forward is refused and the guard names the moved base; the next
  pass then lands both. The brief's own commands run on a throwaway
  repository
  (`defaults.rs::a_late_squash_keeps_what_another_landing_put_on_the_base_branch`),
  and the brief names the merge-base squash and the guard
  (`::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`).
- The published and final-task briefings carry only the task's own values and
  the daemon's `open_pull_request` call, and name no procedure of their own
  beyond the rebase and the push
  (`defaults.rs::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`,
  `::the_final_landing_takes_the_goal_branch_onto_the_base_in_order`).

## Sources

`crates/ariadne-store/src/defaults.rs` (`LANDING_*`, `final_landing_prompt`),
`crates/ariadne-daemon/src/http/landing.rs` (`open_pull_request`,
`verify_merged`), `crates/ariadne-daemon/src/forge/` (`ForgeClient::open`,
`github::open`, `gitlab::open`), `crates/ariadne-daemon/src/gitwt.rs`
(`GitManager::remote_has_branch_tip`),
`crates/ariadne-store/src/tasks.rs` (`set_task_pull_request`,
`clear_task_pull_request`, `TASK_ROWS`, `Store::final_task`,
`Store::start_on_goal_branch`),
`crates/ariadne-store/src/entities.rs`
(`Goal::landing`, `Task::landing_prompt_text`, `Task::pr_url`),
`crates/ariadne-store/src/goals.rs` (`goal_landing`,
`Store::task_landing_branch`),
`crates/ariadne-core/src/lib.rs` (`Landing`),
`crates/ariadne-api/src/tasks.rs` (`OpenPullRequestRequest`).
