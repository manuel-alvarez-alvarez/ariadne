---
id: landing-strategies
status: current
updated: 2026-09-06
areas: [daemon, store, prompts]
commits: [ad268ee0, 305ee064, 45c5e131, 8174c256, 90ac6e67, 524856c7, fdd0c5b6, 23d191a5]
tests:
  - crates/ariadne-daemon/tests/landing_lifecycle.rs
  - crates/ariadne-daemon/tests/repositories.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
---

# Landing strategies

How a task ends. Three endings, one of which lands nothing, run by the agent
that did the work.

## Scope

In: how a task ends and where that is decided, the two merge strategies a
repository takes a change by, the landing briefing a repository carries, the
`direct` and `pull_request` procedures, merge verification and
published-request handling.

Out: who approves the change (004), who agrees the ending (003), and the
state machine around `approved` and `finished` (001).

## Behavior

1. A task ends one of three ways, named by its own `landing`:
   - `merge` — the author puts the change on the base branch itself;
   - `pull_request` — the author publishes a request and somebody else merges
     it;
   - `none` — nothing is landed, and what the task produced is the whole of
     it: a published tag, a filed report, a document that lives elsewhere.
   All three reach `finished` (001). Landing is one way of getting there
   rather than the meaning of being there.
2. A repository takes a change one way, named by its `merge_strategy`:
   `direct` or `pull_request` (default `direct`). That is what a task ends
   with unless whoever wrote the task said otherwise, and the orchestrator
   agrees it with the user task by task (003).
3. The procedure is a **repository field** — the landing briefing. It is
   prefilled from the strategy, can be replaced with text of its own at
   registration or after, and clearing it puts the strategy's default back.
   A briefing naming a placeholder nothing fills in is refused when saved;
   the ones it may name are `{task_title}`, `{branch}`, `{base_branch}` and
   `{repo_path}`.
4. Changing a repository's strategy does not overwrite a landing briefing
   somebody wrote, and does not move a task already planned.
5. A task ending the way its repository takes a change runs that repository's
   own briefing. One ending the other way runs the built-in procedure for that
   way: a text written for direct commits is not one to hand somebody
   publishing a request.
6. An approved task is landed by its own author, in the session and worktree
   it already holds. There is no separate integrator seat.
7. `merge`: rebase the task branch onto the base, squash it into one commit
   with a Conventional Commits subject, fast-forward the base branch in the
   primary checkout, push where there is a remote, then `finish_task` with the
   base branch's sha. The push comes before `finish_task`, because that call
   ends the task and the cleanup behind it takes the worktree.
8. `pull_request`: rebase once — the only rebase — push the branch, and open
   the request with `gh` (github.com) or `glab` (GitLab), whichever the
   `origin` remote calls for, following the repository's own templates. The
   URL is recorded with `record_pull_request`.
9. A published branch only grows: no amend, no rebase, no forced push. A base
   that has moved is merged in and pushed plainly.
10. The author waits on its own published request in its own session, polling
   the forge and sleeping between polls, capped at five minutes a call so the
   session keeps reporting activity (009). It answers every comment, and a
   change somebody asks for is made on the branch and put through the Ariadne
   reviewers before it is pushed (004).
11. Once the request is approved and green it is merged with `--squash`, the
   base branch is fast-forwarded in the primary checkout, and the sha is
   reported with `finish_task`. A request closed unmerged ends the task with
   `fail_task`.
12. `none`: the author is briefed to check that what the task asked for is
    where the task said to put it, and that nothing is left only in the
    worktree, which is thrown away with the task. Then `finish_task`, with no
    merge commit.
13. The daemon accepts a merge sha only after verifying it with
    `git merge-base --is-ancestor`: a merge that never happened is refused. A
    task that lands nothing has nothing git can be asked about, so nothing is
    verified and no sha is demanded.
14. The forge is read off the `origin` remote at landing time rather than
    configured anywhere, so the answer cannot go stale.

## Acceptance criteria

- An approved task is landed by its own author
  (`landing_lifecycle.rs::an_approved_task_is_landed_by_its_own_author`) and
  is briefed with the repository's own landing text
  (`::an_approved_author_is_briefed_with_the_repositorys_own_landing_text`).
- A merge that never happened is refused
  (`landing_lifecycle.rs::a_merge_that_never_happened_is_refused`), and a
  squashed request lands on the sha the author fast-forwarded to
  (`::a_squashed_request_lands_on_the_sha_the_author_fast_forwarded_to`).
- A new repository lands by its strategy's briefing unless it was given one
  (`repositories.rs::a_new_repository_lands_by_its_strategys_briefing_unless_it_was_given_one`),
  the briefing survives a strategy change
  (`::the_landing_briefing_is_set_reset_and_kept_across_a_strategy_change`), and
  an unknown placeholder is a 400
  (`::a_landing_briefing_with_an_unknown_placeholder_is_a_400`).
- Each strategy's briefing is one procedure and nothing of the other
  (`defaults.rs::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`),
  and nothing the author still has to run comes after `finish_task`
  (`defaults.rs::nothing_the_author_still_has_to_run_comes_after_the_call_that_ends_the_task`).
- How a task ends travels as the three endings there are
  (`edit.rs::how_the_task_ends_travels_as_the_three_endings_there_are`).

## Sources

`crates/ariadne-store/src/defaults.rs` (`LANDING_*`),
`crates/ariadne-store/src/entities.rs` (`Task::landing_prompt_text`),
`crates/ariadne-daemon/src/http/landing.rs`,
`crates/ariadne-core/src/lib.rs` (`Landing`, `MergeStrategy`).
