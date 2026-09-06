---
id: how-a-task-ends
status: current
updated: 2026-09-06
areas: [daemon, store, prompts]
commits: [ad268ee0, 305ee064, 45c5e131, 8174c256, 90ac6e67, 524856c7, fdd0c5b6, a69b953f, 29e6d84e, f79c8e15, a4d7da95]
tests:
  - crates/ariadne-daemon/tests/landing_lifecycle.rs
  - crates/ariadne-daemon/tests/repositories.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
---

# How a task ends

Three endings, one of which lands nothing, run by the agent that did the work.

## Scope

In: the three endings and where each is decided, the procedure of each,
merge verification, and published-request handling.

Out: who approves the change (004), who agrees the ending (003), and the
state machine around `approved` and `finished` (001).

## Behavior

1. A task ends one of three ways, named by its own `landing`:
   - `merge` — the author puts the change on the base branch itself;
   - `pull_request` — the author opens a request and sees it through: it
     answers what is written on it, and the task ends when the request is
     merged;
   - `none` — nothing is landed, and what the task produced is the whole of
     it: a published tag, a filed report, a document that lives elsewhere.
   All three reach `finished` (001). Landing is one way of getting there
   rather than the meaning of being there.
2. `merge` is what a task ends with unless whoever wrote it said otherwise.
   The orchestrator agrees the ending with the user task by task (003), and
   the user can move it while the task is still pending or ready.
3. The procedure is Ariadne's, one text per ending, and nothing overrides it.
   A repository has no say: it is a checkout and a base branch (002), and a
   second answer stored there could only disagree with the task's. Each text
   is rendered with `{task_title}`, `{branch}`, `{base_branch}` and
   `{repo_path}`, and may name nothing else.
4. An approved task is landed by its own author, in the session and worktree
   it already holds. There is no separate integrator seat.
5. `merge`: rebase the task branch onto the base, squash it into one commit
   with a Conventional Commits subject, fast-forward the base branch in the
   primary checkout, push where there is a remote, then `finish_task` with the
   base branch's sha. The push comes before `finish_task`, because that call
   ends the task and the cleanup behind it takes the worktree.
6. `pull_request`: rebase once — the only rebase — push the branch, and open
   the request with `gh` (github.com) or `glab` (GitLab), whichever the
   `origin` remote calls for, following the repository's own templates. The
   URL is recorded with `record_pull_request`.
7. A published branch only grows: no amend, no rebase, no forced push. A base
   that has moved is merged in and pushed plainly.
8. The author waits on its own published request in its own session, polling
   the forge and sleeping between polls, capped at five minutes a call so the
   session keeps reporting activity (009). It answers every comment, and a
   change somebody asks for is made on the branch and put through the Ariadne
   reviewers before it is pushed (004).
9. Once the request is approved and green it is merged with `--squash`, the
   base branch is fast-forwarded in the primary checkout, and the sha is
   reported with `finish_task`. A request closed unmerged ends the task with
   `fail_task`.
10. `none`: the author is briefed to check that what the task asked for is
    where the task said to put it, and that nothing is left only in the
    worktree, which is thrown away with the task. Then `finish_task`, with no
    merge commit.
11. The daemon accepts a merge sha only after verifying it with
    `git merge-base --is-ancestor`: a merge that never happened is refused. A
    task that lands nothing has nothing git can be asked about, so nothing is
    verified and no sha is demanded.
12. The forge is read off the `origin` remote at landing time rather than
    configured anywhere, so the answer cannot go stale.

## Acceptance criteria

- An approved task is landed by its own author
  (`landing_lifecycle.rs::an_approved_task_is_landed_by_its_own_author`), with
  the procedure of the ending it carries
  (`prompts.rs::the_task_says_what_the_author_lands_with`,
  `store.rs::a_task_lands_by_the_ending_it_carries_and_the_repository_has_no_say`).
- A merge that never happened is refused
  (`landing_lifecycle.rs::a_merge_that_never_happened_is_refused`), and a
  squashed request lands on the sha the author fast-forwarded to
  (`::a_squashed_request_lands_on_the_sha_the_author_fast_forwarded_to`).
- A repository takes nothing about how work ends in it, and the endpoint that
  listed the strategies is gone
  (`repositories.rs::a_repository_refuses_anything_about_how_work_ends`).
- Each ending's briefing is one procedure and nothing of the other
  (`defaults.rs::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`),
  and nothing the author still has to run comes after `finish_task`
  (`defaults.rs::nothing_the_author_still_has_to_run_comes_after_the_call_that_ends_the_task`).
- How a task ends travels as the three endings there are
  (`edit.rs::how_the_task_ends_travels_as_the_three_endings_there_are`).

## Sources

`crates/ariadne-store/src/defaults.rs` (`LANDING_*`),
`crates/ariadne-store/src/entities.rs` (`Task::landing_prompt_text`),
`crates/ariadne-daemon/src/http/landing.rs`,
`crates/ariadne-core/src/lib.rs` (`Landing`).
