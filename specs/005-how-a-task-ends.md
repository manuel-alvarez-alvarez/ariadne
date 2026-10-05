---
id: how-a-task-ends
status: current
updated: 2026-10-05
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
verification, and published-request handling.

Out: who approves the change (004), and the state machine around `approved`
and `finished` (001).

## Behavior

1. A task ends the way its goal's `landing` names. The user chooses it once,
   when the goal is created, and every task of the goal follows it:
   - `merge` — the author puts the change on the base branch itself;
   - `pull_request` — the author opens a request and sees it through: it
     answers what is written on it, waits for a human to merge it, and the
     task ends once that merge lands;
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
6. `pull_request`: rebase once — the only rebase — push the branch, and open
   the request with `gh` (github.com) or `glab` (GitLab), whichever the
   `origin` remote calls for, following the repository's own templates. The
   URL is recorded with `record_pull_request`. The whole procedure — the
   forge, the publish, the poll, the readiness report and the merge — is the
   `pull-request` skill's (017), and the briefing carries only the task's own
   values and a pointer to it.
7. A published branch only grows: no amend, no rebase, no forced push. A base
   that has moved is merged in and pushed plainly.
8. The author waits on its own published request in its own session, polling
   the forge and sleeping between polls, capped at five minutes a call so the
   session keeps reporting activity (009). It answers every comment, and a
   change somebody asks for is made on the branch and put through the Ariadne
   reviewers before it is pushed (004).
9. Publishing a request is not the same as it being ready to merge.
   `record_pull_request` takes a `ready` flag beside the URL, false by
   default, so the URL alone never raises `waiting_user`. The author sets it
   true once every required approval and check reads green, which raises
   `waiting_user` on that one transition — a repeated `true` changes nothing,
   and a later `false` takes it back down, so a check that fails after an
   earlier ready report stops claiming the request is the user's and a later
   ready report raises the notice again. Ariadne never merges the request: a
   human does, once it is ready. The author waits for that merge the same way
   it waited for the checks and the comments, then fast-forwards the base
   branch in the primary checkout and reports the sha with `finish_task`. A
   request closed unmerged ends the task with `fail_task`. A restart that
   resumes the author while the request still reads ready raises
   `waiting_user` again, the same way one that merely published it once did
   (009).
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
    final task. It lands on the
    repository base branch. Its landing briefing is a text of its own, naming
    the goal branch and the base branch and pointing at the `pull-request`
    skill (017), which runs the same procedure as an ordinary task's request
    and additionally deletes the goal branch, local and remote, once the
    merge lands. A cleanup never deletes the goal branch before that.
11. `none`: the author is briefed to check that what the task asked for is
    where the task said to put it, and that nothing is left only in the
    worktree, which is thrown away with the task. Then `finish_task`, with no
    merge commit.
12. The daemon accepts a merge sha only after verifying it with
    `git merge-base --is-ancestor`, against the ending of the task's goal: a
    merge that never happened is refused. The final task of a
    `feature_branch` goal is verified as a published request is: its sha has
    to be on the repository base branch. A
    task that lands nothing has nothing git can be asked about, so nothing is
    verified and no sha is demanded.
13. The forge is read off the `origin` remote at landing time rather than
    configured anywhere, so the answer cannot go stale.

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
  finished, then starts on the goal branch and cuts no branch. Its landing
  briefing names the goal branch as the head and the base branch as the
  target, and the daemon accepts the squashed sha on the base branch and
  refuses the goal branch tip
  (`final_tasks.rs::the_final_task_waits_then_lands_the_goal_branch_on_the_base`).
  The briefing runs its eight steps in order and nothing after `finish_task`
  (`defaults.rs::the_final_landing_takes_the_goal_branch_onto_the_base_in_order`,
  `::nothing_the_author_still_has_to_run_comes_after_the_call_that_ends_the_task`).
- A merge that never happened is refused
  (`landing_lifecycle.rs::a_merge_that_never_happened_is_refused`), and a
  squashed request lands on the sha the author fast-forwarded to
  (`::a_squashed_request_lands_on_the_sha_the_author_fast_forwarded_to`).
- A repository supplies the landing for a new goal without one, and editing
  the default changes no existing goal
  (`repositories.rs::a_repository_defaults_new_goals_without_changing_existing_ones`).
- Each ending's briefing is one procedure and nothing of the other
  (`defaults.rs::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`),
  and nothing the author still has to run comes after `finish_task`
  (`defaults.rs::nothing_the_author_still_has_to_run_comes_after_the_call_that_ends_the_task`).
- No landing briefing names a forge merge command: a human merges every
  request, never the author
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
- The published and final-task briefings carry the task's values and point
  at the `pull-request` skill rather than spelling out its procedure, which
  the skill itself carries in order
  (`defaults.rs::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`,
  `::the_final_landing_takes_the_goal_branch_onto_the_base_in_order`).
- Publication alone leaves the task nobody's to merge; a ready report raises
  `waiting_user` on the change into it, and a repeat of the same report
  raises nothing again
  (`landing_lifecycle.rs::a_squashed_request_lands_on_the_sha_the_author_fast_forwarded_to`).
  A restart of the author while the request still reads ready raises it
  again
  (`scheduler_attention.rs::a_published_task_still_says_the_merge_is_the_users_after_its_author_is_resumed`).
  The store answers whether a readiness report changed anything
  (`store.rs::a_tasks_readiness_report_says_whether_it_changed`).

## Sources

`crates/ariadne-store/src/defaults.rs` (`LANDING_*`,
`final_landing_prompt`, `PULL_REQUEST_SKILL`),
`crates/ariadne-store/skills/pull-request/SKILL.md`,
`crates/ariadne-daemon/src/http/landing.rs` (`record_pull_request`),
`crates/ariadne-store/src/tasks.rs` (`set_task_pull_request_ready`),
`crates/ariadne-store/src/entities.rs`
(`Goal::landing`, `Task::landing_prompt_text`, `Task::pr_ready`),
`crates/ariadne-store/src/goals.rs` (`goal_landing`,
`Store::task_landing_branch`), `crates/ariadne-store/src/tasks.rs`
(`TASK_ROWS`, `Store::final_task`, `Store::start_on_goal_branch`),
`crates/ariadne-daemon/src/http/landing.rs`,
`crates/ariadne-core/src/lib.rs` (`Landing`).
