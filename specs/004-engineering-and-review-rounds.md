---
id: engineering-and-review-rounds
status: current
updated: 2026-09-04
areas: [daemon, store, prompts]
commits: [ad268ee0, 2ca6dd29, 88bf39ac, da10e748, b21bd69e]
tests:
  - crates/ariadne-daemon/tests/prompts.rs
  - crates/ariadne-daemon/tests/resume.rs
  - crates/ariadne-daemon/tests/landing_lifecycle.rs
  - crates/ariadne-store/tests/store.rs
---

# Engineering and review rounds

What happens between a task becoming `ready` and being `approved`: one
engineer, one or more reviewers, and as many rounds as the change needs.

## Scope

In: the engineer session and what it owns, `request_review`, reviewer
sessions, the verdict per round, how a round closes, and how the two sides
are resumed between rounds.

Out: the transition table itself (001), the landing that follows approval
(005), and the briefings' wording (006).

## Behavior

1. A `ready` task gets one engineer session, in its own worktree on its own
   branch, and moves to `in_progress`.
2. The task never leaves that engineer: the same session and worktree carry it
   from the first commit through every review round to the merge.
3. The engineer implements only its task, obeys the repository's conventions
   files, keeps tests and linters green, and writes no authorship or tool
   trailer in its commits.
4. A task the engineer cannot do as written ends with its own `fail_task` and
   the reason on the task.
5. `request_review` moves the task to `under_review` and carries one short
   summary — what changed, why, and how it was verified. That summary is what
   the reviewers read first.
6. Each assigned reviewer profile gets one session for the whole task, in a
   detached read-only worktree (002). A round is not part of a reviewer's
   identity, only of the briefing it is woken with.
7. A reviewer verifies the change in its own worktree — installing what it
   needs, building, testing and linting there — and gives exactly one verdict
   per round through `submit_verdict`. Nothing else counts as a verdict.
8. Verdicts close a round before anything else is done with it: any request
   for changes moves the task to `changes_requested`, whatever else the round
   holds. Otherwise, approvals of the round are counted and the task is
   `approved` once they reach the goal's `required_approvals`.
9. A `changes_requested` task resumes its engineer with the round's feedback,
   under a heading naming who wrote each point — the Ariadne reviewers, or the
   people on a published request (005). The engineer answers every point and
   says why where the code stays.
10. A reviewer that has already voted this round is nobody's blocker: no
    attention is raised on it and no session is started for it.
11. An engineer whose task is under review is likewise not the agent the work
    is waiting on (009).

## Acceptance criteria

- A spawned engineer is briefed from the built-in template, word for word
  (`prompts.rs::a_spawned_engineer_is_briefed_from_the_builtin_template`,
  `::a_spawn_assembles_the_default_briefing_word_for_word`).
- A resume and a review round assemble word for word
  (`prompts.rs::a_resume_and_a_review_round_assemble_word_for_word`), and the
  reviewer is briefed with the summary review was requested with
  (`::a_reviewer_is_briefed_with_the_summary_review_was_requested_with`).
- The engineer keeps one session across review rounds
  (`resume.rs::resuming_the_engineer_reuses_its_session_across_review_rounds`),
  and so does each reviewer (`::a_reviewer_reuses_its_session_across_review_rounds`);
  a reviewer with no agent id is spawned afresh
  (`::a_reviewer_without_an_agent_id_is_spawned_afresh`).
- One verdict per reviewer per round is recorded
  (`store.rs::one_review_verdict_per_round`).
- The review summary is the reason of the latest review request
  (`store.rs::the_review_summary_is_the_reason_of_the_latest_review_request`).
- A reviewer that already voted raises no attention
  (`events.rs::a_reviewer_that_already_voted_raises_no_attention`).
- A revision of a published request goes back to the reviewers
  (`landing_lifecycle.rs::a_revision_of_a_published_request_goes_back_to_the_reviewers`).

## Sources

`crates/ariadne-daemon/src/scheduler/tasks.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-store/src/defaults.rs` (`ENGINEER_*`, `REVIEWER_*`,
`CHANGES_REQUESTED`).
