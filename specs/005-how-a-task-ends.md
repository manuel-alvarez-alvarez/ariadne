---
id: how-a-task-ends
status: current
updated: 2026-10-07
areas: [daemon, store, prompts]
commits: [ad268ee0, 305ee064, 45c5e131, 8174c256, 90ac6e67, 524856c7, fdd0c5b6, a69b953f, 29e6d84e, f79c8e15, a4d7da95]
tests:
  - crates/ariadne-daemon/tests/it/landing_lifecycle.rs
  - crates/ariadne-daemon/tests/it/final_tasks.rs
  - crates/ariadne-daemon/tests/it/kept_requests.rs
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
   - `pull_request` — the author opens a request and sees it through: it
     answers what is written on it, keeps its checks and its branch current,
     and the task ends once a human merges it (026);
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
   `open_pull_request`, titled by the repository's commit conventions and
   with its body from the repository's request template. The daemon opens
   the request through `gh` or `glab` itself (rule 13), and starts its work
   on it: a row that keeps Ariadne's bookkeeping of the request, and none of
   what the forge holds (026). It refuses where the repository's forge integration is
   off: nothing would then read the request for its author. The task stays
   `approved`, and its author keeps the request from there with the
   `pr-babysit` skill, which the daemon loads for every author of a task that
   lands by request (017). The landing briefing names the skill and no
   `finish_task`.
7. A published branch only grows: no amend, no rebase, no forced push. A base
   that has moved is merged in and pushed plainly.
8. The author never polls. The daemon reads the forge (026, 027) and hands
   the author's own session the news of its request in one prompt: new
   comments by another login, failed checks, a base ahead of the head, a new
   review decision, and a merge or a close. An idle author on an open request
   waits on the forge, and is never nudged for it (009 rule 41). A change a
   comment or a check asks for is made on the branch and put through the
   Ariadne reviewers with `request_review` before it is pushed (004): the
   task goes back to `under_review`, and once it is approved again the author
   is picked up with the keep-request briefing, which pushes the branch and
   reads the request. The same briefing picks up an author whose agent went
   away while its request is open.
9. Publishing a request is not the same as it being ready to merge. The
   author reports `ready: true` with `report_pull_request` once every required
   approval and check reads green, which raises `waiting_user` on its session
   on that one change; a later `ready: false` takes it back down. Ariadne
   never merges the request: a human does. On the merge the author
   fast-forwards the base branch in the primary checkout and reports the sha
   with `finish_task`. Where the task is still `approved` after that, the
   daemon finishes it itself, as `daemon`: once the author's turn that read
   the merge has ended, once the author has been quiet for the quiet nudge
   since it was told, or at once where no author is up. The merge commit is
   the request's own, as the forge reports it (`pull_requests.merge_sha`,
   migration `0017`: GitHub's `mergeCommit`, GitLab's merge or squash
   commit, or its head after a fast-forward merge). Before the task ends
   the daemon fetches the base from the remote, checks that the merge
   commit is on it, and fast-forwards the local base branch to it: with
   `git merge --ff-only` in the worktree that has it checked out, else
   `update-ref`. A base with local commits the remote lacks, a local change
   the fast-forward would overwrite, or a failed fetch leaves the task
   `approved`, and the next pass tries again: a task finished on a base
   that lacks its change would start its dependents without it. A row the
   forge named no merge commit for ends on its head where the head is on
   the base, else on the fetched tip. The task's cleanup then stops the
   author and its agent (009). A request closed unmerged ends the task with
   `fail_task`. A restart that resumes the author while its open request
   still reads ready raises `waiting_user` again (009).
10. `feature_branch` ends in a final task per repository: the one live task
    that depends directly on every other live task of that repository. Two
    such tasks would depend on each other, so at most one matches. It needs
    no reviewer. A cancelled task counts nowhere in that match: not as the
    task itself, not among the dependencies it carries, and not among the
    other tasks it must depend on. Cancelled is terminal and never retried,
    so counting one anywhere there would block every repository's final task
    from matching again.
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
    branch and skips the rebase every other task already did. Its author
    keeps that request as any other task's author keeps one, and the task
    ends on its merge; the goal branch itself is deleted, local and remote,
    by the daemon once the task is over (026).
11. `none`: the author is briefed to check that what the task asked for is
    where the task said to put it, and that nothing is left only in the
    worktree, which is thrown away with the task. Then `finish_task`, with no
    merge commit.
12. `finish_task` is verified against how the task ends. `merge` and
    `feature_branch` accept a sha only once git proves it, with
    `git merge-base --is-ancestor`, an ancestor of the landing branch: a
    merge that never happened is refused. `pull_request` accepts it once the
    forge, read at the call, says the task's request merged — no sha is asked onto
    the base branch, since the daemon never merges the request itself, and
    the head branch may be gone from the remote by then. The final task of a
    `feature_branch` goal is verified the same way, once it works on the
    goal branch. A task that lands nothing has nothing git can be asked
    about, so nothing is verified and no sha is demanded.
13. The forge is detected off the repository's remote and stored on it, not
    read fresh at landing time (025): a `forge_integrations` row, enabled or
    not, is what `open_pull_request` needs to find one, and the host it
    names is what the CLI's authentication is checked against. The daemon
    runs the forge's own CLI, `gh` or `glab`, itself — never the agent.
    Every call names that host: `gh pr create --repo <host>/<owner>/<name>`,
    and `glab mr create -R https://<host>/<group>/<name>`, since `-R` reads
    a bare `host/group/name` as a group path on its default host.

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
- A cancelled task counts nowhere in the final task match: a cancelled task
  the final task never depended on does not stop a plan from finalizing, and
  the final task still starts once the live task it depends on is finished
  (`final_tasks.rs::a_cancelled_task_does_not_block_finalize_or_the_final_task_starting`,
  `store.rs::final_task_ignores_a_cancelled_sibling`). A candidate's own
  dependency on a cancelled task is excluded the same way
  (`store.rs::final_task_ignores_a_cancelled_dependency`), and a cancelled
  task is never the match itself, even where the count would otherwise agree
  (`store.rs::final_task_is_never_a_cancelled_candidate`).
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
  request for the goal branch onto the base branch, its author keeps it with
  `pr-babysit`, and the task finishes only once the request merged; the
  daemon then deletes the goal branch, local and remote
  (`final_tasks.rs::the_final_task_waits_then_lands_the_goal_branch_on_the_base`).
  The briefing runs its steps in order: the push, `open_pull_request` and
  the skill that keeps the request, and no `finish_task`
  (`defaults.rs::the_final_landing_takes_the_goal_branch_onto_the_base_in_order`,
  `::nothing_the_author_still_has_to_run_comes_after_the_call_that_ends_the_task`).
- A merge that never happened is refused
  (`landing_lifecycle.rs::a_merge_that_never_happened_is_refused`).
- `open_pull_request` on an approved, pushed task of a repository with a
  detected forge runs the stub's `pr create` with the head, the base, the
  title and the body, stores the URL, and answers it; a second call answers
  the same URL and the stub sees no second create. An unpushed branch is
  refused with 409 that says to push first
  (`landing_lifecycle.rs::opening_a_pull_request_runs_the_forge_cli_once_and_keeps_the_task_until_the_merge`).
  A repository whose integration is off is refused with 409 `forge_disabled`
  and opens nothing
  (`::opening_a_pull_request_refuses_with_the_integration_off`). A
  repository with no detected forge is refused with 409
  (`::opening_a_pull_request_refuses_with_no_forge_detected`), and a stub
  whose `auth status` exits 1 is refused with 409 and the probe's message
  (`::opening_a_pull_request_refuses_an_unauthenticated_cli`).
- A `pull_request` task stays `approved` while its request is open, its
  author loads `pr-babysit`, and `finish_task` is refused until the request's
  row reads `merged`, then accepted with no sha asked onto the base branch
  (`landing_lifecycle.rs::opening_a_pull_request_runs_the_forge_cli_once_and_keeps_the_task_until_the_merge`,
  `kept_requests.rs::a_merge_is_told_to_the_author_and_then_ends_the_task_and_its_agent`).
  After the merge news, the daemon finishes the task itself, the author is
  stopped and the request's cleanup is recorded
  (`kept_requests.rs::a_merge_is_told_to_the_author_and_then_ends_the_task_and_its_agent`).
  A merge done while the author is down ends the task on the request's own
  merge commit, not on a later tip, once the local base is fast-forwarded
  to the remote's
  (`kept_requests.rs::a_merge_ends_the_task_on_its_own_merge_commit_once_the_local_base_holds_it`).
  A close is told to the author and finishes nothing
  (`kept_requests.rs::a_close_is_told_to_the_author_and_finishes_nothing`).
- A revision of an open request goes back to the reviewers, opens no second
  request, and the author is picked up with the keep-request briefing once
  approved again
  (`landing_lifecycle.rs::a_revision_of_a_published_request_goes_back_to_the_reviewers`).
- An open request no report called ready raises no attention on its own, and
  a resumed author carries none forward
  (`scheduler_attention.rs::an_open_pull_request_raises_no_attention`).
- A repository supplies the landing for a new goal without one, and editing
  the default changes no existing goal
  (`repositories.rs::a_repository_defaults_new_goals_without_changing_existing_ones`).
- Each ending's briefing is one procedure and nothing of the other
  (`defaults.rs::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`),
  and nothing the author still has to run comes after `finish_task`, while a
  landing by request names no `finish_task` at all
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
