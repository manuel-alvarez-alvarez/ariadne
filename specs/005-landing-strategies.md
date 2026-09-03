---
id: landing-strategies
status: current
updated: 2026-09-04
areas: [daemon, store, prompts]
commits: [ad268ee0, 305ee064, 45c5e131, 8174c256, 90ac6e67, 524856c7, fdd0c5b6]
tests:
  - crates/ariadne-daemon/tests/landing_lifecycle.rs
  - crates/ariadne-daemon/tests/repositories.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
---

# Landing strategies

How a finished change reaches a base branch. One procedure per repository,
run by the agent that wrote the change.

## Scope

In: the two merge strategies, the landing briefing a repository carries, the
`direct` and `pull_request` procedures, merge verification, published-request
handling, and the planner's spec landing.

Out: who approves the change (004), and the state machine around `approved`
and `merged` (001).

## Behavior

1. A repository lands one way, named by its `merge_strategy`: `direct` or
   `pull_request` (default `direct`).
2. The procedure is a **repository field** — the landing briefing. It is
   prefilled from the strategy, can be replaced with text of its own at
   registration or after, and clearing it puts the strategy's default back.
   A briefing naming a placeholder nothing fills in is refused when saved;
   the ones it may name are `{task_title}`, `{branch}`, `{base_branch}` and
   `{repo_path}`.
3. Changing a repository's strategy does not overwrite a landing briefing
   somebody wrote.
4. An approved task is landed by its own engineer, in the session and worktree
   it already holds. There is no separate integrator role.
5. `direct`: rebase the task branch onto the base, squash it into one commit
   with a Conventional Commits subject, fast-forward the base branch in the
   primary checkout, push where there is a remote, then `mark_merged` with the
   base branch's sha. The push comes before `mark_merged`, because that call
   ends the task and the cleanup behind it takes the worktree.
6. `pull_request`: rebase once — the only rebase — push the branch, and open
   the request with `gh` (github.com) or `glab` (GitLab), whichever the
   `origin` remote calls for, following the repository's own templates. The
   URL is recorded with `record_pull_request`.
7. A published branch only grows: no amend, no rebase, no forced push. A base
   that has moved is merged in and pushed plainly.
8. The engineer waits on its own published request in its own session, polling
   the forge and sleeping between polls, capped at five minutes a call so the
   session keeps reporting activity (009). It answers every comment, and a
   change somebody asks for is made on the branch and put through the Ariadne
   reviewers before it is pushed (004).
9. Once the request is approved and green it is merged with `--squash`, the
   base branch is fast-forwarded in the primary checkout, and the sha is
   reported with `mark_merged`. A request closed unmerged ends the task with
   `fail_task`.
10. The daemon accepts a merge sha only after verifying it with
    `git merge-base --is-ancestor`: a merge that never happened is refused.
11. The forge is read off the `origin` remote at landing time rather than
    configured anywhere, so the answer cannot go stale.
12. The planner lands its approved spec by the same strategies, with a
    procedure of Ariadne's own (003): `direct` commits it on the base branch of
    the primary checkout after checking that checkout is on the base branch;
    `pull_request` holds the spec branch in a throwaway worktree, sees the
    request through and merges it. A repository's rewritten landing briefing
    does not change how its spec lands.

## Acceptance criteria

- An approved task is landed by its own engineer
  (`landing_lifecycle.rs::an_approved_task_is_landed_by_its_own_engineer`) and
  is briefed with the repository's own landing text
  (`::an_approved_engineer_is_briefed_with_the_repositorys_own_landing_text`).
- A merge that never happened is refused
  (`landing_lifecycle.rs::a_merge_that_never_happened_is_refused`), and a
  squashed request lands on the sha the engineer fast-forwarded to
  (`::a_squashed_request_lands_on_the_sha_the_engineer_fast_forwarded_to`).
- A new repository lands by its strategy's briefing unless it was given one
  (`repositories.rs::a_new_repository_lands_by_its_strategys_briefing_unless_it_was_given_one`),
  the briefing survives a strategy change
  (`::the_landing_briefing_is_set_reset_and_kept_across_a_strategy_change`), and
  an unknown placeholder is a 400
  (`::a_landing_briefing_with_an_unknown_placeholder_is_a_400`).
- Each strategy's briefing is one procedure and nothing of the other
  (`defaults.rs::each_landing_briefing_is_one_strategy_and_nothing_of_the_other`),
  and nothing the engineer still has to run comes after `mark_merged`
  (`defaults.rs::nothing_the_engineer_still_has_to_run_comes_after_the_call_that_ends_the_task`).
- The spec landing names no task tool
  (`defaults.rs::a_spec_landing_names_no_task_tool`) and lands the way the
  repository takes a change
  (`defaults.rs::the_spec_lands_the_way_its_repository_takes_a_change`).
- A rewritten landing briefing does not change how the spec lands
  (`prompts.rs::a_rewritten_landing_briefing_does_not_change_how_the_spec_lands`).

## Sources

`crates/ariadne-store/src/defaults.rs` (`LANDING_*`, `SPEC_LANDING_*`),
`crates/ariadne-daemon/src/http/landing.rs`,
`crates/ariadne-core/src/lib.rs`
(`MergeStrategy`).
