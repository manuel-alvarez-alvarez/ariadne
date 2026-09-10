---
id: authoring-and-review
status: current
updated: 2026-09-10
areas: [daemon, store, prompts]
commits: [ad268ee0, 2ca6dd29, 88bf39ac, da10e748, b21bd69e, a69b953f, 03f9c8b7, 29e6d84e, 1b09ac10]
tests:
  - crates/ariadne-daemon/tests/prompts.rs
  - crates/ariadne-daemon/tests/resume.rs
  - crates/ariadne-daemon/tests/landing_lifecycle.rs
  - crates/ariadne-daemon/tests/multi_author_tasks.rs
  - crates/ariadne-store/tests/store.rs
---

# Authoring and review

What happens between a task becoming `ready` and being `approved`: the
authors — one for most tasks — one or more reviewers, and as many reviews as
each change needs.

## Scope

In: the author sessions and what each owns, `request_review`, reviewer
sessions, the verdict each reviewer owes, how a review settles, how the two
sides are resumed between reviews, and — on a task staffed with several
authors — the pick that names the change that lands.

Out: the transition table itself (001), the landing that follows approval
(005), and the briefings' wording (006).

## Behavior

1. A `ready` task gets one author session per staffed author, each in its
   own worktree on its own branch (002), and moves to `in_progress`. Most
   tasks staff one author; several authors write the same task side by
   side, each on its own model, and need at least one reviewer (017).
2. The task never leaves its authors: the same sessions and worktrees carry
   it from the first commit through every review to the merge.
3. The author implements only its task, obeys the repository's conventions
   files, keeps tests and linters green, and writes no authorship or tool
   trailer in its commits.
4. A task the author cannot do as written ends with its own `fail_task` and
   the reason on the task.
5. `request_review` moves the task to `under_review` and carries one short
   summary — what changed, why, and how it was verified. That summary is what
   the reviewers read first.
6. Each reviewer the task staffs (017) gets one session for the whole task, in
   a detached read-only worktree (002). Which review it is on is not part of a
   reviewer's identity, only of the briefing it is woken with.
7. A reviewer verifies the change in its own worktree — installing what it
   needs, building, testing and linting there — and gives exactly one verdict
   through `submit_verdict` on each review it is asked for. Nothing else
   counts as a verdict. Anything it cannot judge from the change it asks the
   author about instead (018); a question is not a verdict, and asking one
   settles nothing.
8. There are no numbered rounds. A review is bounded by the request that
   opened it: the verdicts that count are the ones sent since the author last
   asked, and asking again supersedes everything said about the change before
   it. That boundary is a row of the channel (018) rather than a counter, so
   nothing has to be reset.
9. Verdicts settle a review before anything else is done with it: any request
   for changes moves the task to `changes_requested`, whatever else the review
   holds. Otherwise the approvals are counted and the task is `approved` once
   every reviewer staffed on it has approved. A task staffed with none is
   approved the moment its author asks: there is nobody to ask (017).
10. A `changes_requested` task resumes its author with the feedback, under a
    heading naming who wrote each point — the Ariadne reviewers, or the people
    on a published request (005). The author answers every point and says why
    where the code stays.
11. A reviewer that has already voted on the open review is nobody's blocker:
    no attention is raised on it and no session is started for it. It stays up
    all the same, until the task is over, because the author may still have
    something to ask it (018).
12. An author whose task is under review is likewise not the agent the work
    is waiting on (009).
13. On a task staffed with several authors the reviews run side by side, one
    per author. The task is `under_review` from the first `request_review`
    to the pick; a later author's request opens its own review on the
    channel without moving the task, and a verdict is addressed to the
    author whose change it judges — one per reviewer per author's open
    review, and none for an author that has not asked. A change request
    reaches that author as a message, and only that author revises. A
    reviewer works one review at a time, oldest first by author order, and
    each verdict it gives is the event that hands it the next: a reviewer
    whose pane survived the last review is briefed for the next one the
    moment it owes it — the full briefing naming the author and its branch,
    typed into the live pane, with its detached worktree moved to that
    branch first — rather than waiting on the quiet clock (009). That
    briefing is also the review request's delivery: a contested request is
    never typed to a reviewer as a bare message, since the summary alone
    names neither the author nor the branch, and the briefing is what
    stamps it delivered on the channel (018).
14. Approval says a change is sound; with several sound changes, the pick
    says which one lands. Once every author is approved, each reviewer is
    asked to pick a winner — `pick_winner`, once per reviewer, refused by
    name a second time and refused entirely before every author is approved.
    When every staffed reviewer has picked, the author with the most picks
    wins; a tie goes to the author listed first. The task then moves to
    `approved` with the winner recorded on it, the losing authors' sessions,
    worktrees and branches are removed, and the winner lands the task as a
    lone author would (005). The winner is written before anything else of
    the settlement, and the rest is idempotent: a daemon that dies between
    the two leaves a task `under_review` with a winner on it, which the next
    pass — a restart's first included — routes straight back through the
    settlement.

## Acceptance criteria

- A spawned author is briefed from the built-in template, word for word
  (`prompts.rs::a_spawned_author_is_briefed_from_the_builtin_template`,
  `::a_spawn_assembles_the_default_briefing_word_for_word`).
- A resume and a review assemble word for word
  (`prompts.rs::a_resume_and_a_review_assemble_word_for_word`), and the
  reviewer is briefed with the summary review was requested with
  (`::a_reviewer_is_briefed_with_the_summary_review_was_requested_with`).
- The author keeps one session across reviews
  (`resume.rs::resuming_the_author_reuses_its_session_across_reviews`),
  and so does each reviewer (`::a_reviewer_reuses_its_session_across_reviews`);
  a reviewer with no agent id is spawned afresh
  (`::a_reviewer_without_an_agent_id_is_spawned_afresh`).
- A verdict belongs to the review that was asked for, and asking again
  supersedes what came before it
  (`store.rs::a_verdict_belongs_to_the_review_that_was_asked_for`); a second
  verdict on the open review is refused by name
  (`agent_messages.rs::only_one_verdict_per_reviewer_per_review_is_taken`).
- The review summary is the reason of the latest review request
  (`store.rs::the_review_summary_is_the_reason_of_the_latest_review_request`).
- A reviewer that already voted raises no attention
  (`events.rs::a_reviewer_that_already_voted_raises_no_attention`).
- A revision of a published request goes back to the reviewers
  (`landing_lifecycle.rs::a_revision_of_a_published_request_goes_back_to_the_reviewers`).
- A task staffed with two authors runs two sessions in two worktrees
  (`multi_author_tasks.rs::a_task_staffed_with_two_authors_runs_two_sessions_in_two_worktrees`).
- The pick starts only after every author is approved
  (`multi_author_tasks.rs::the_pick_starts_only_after_every_author_is_approved`),
  a second pick from the same reviewer is refused by name
  (`::a_second_pick_from_the_same_reviewer_is_refused_by_name`), and exactly
  one branch lands with the losers gone after the landing
  (`::exactly_one_branch_lands_and_the_losers_are_gone`).
- A live reviewer is briefed for the next author's review without the quiet
  clock, its worktree moved to that author's branch first
  (`multi_author_tasks.rs::a_live_reviewer_is_briefed_for_the_next_author_without_the_quiet_clock`),
  a contested review request reaches it only as that briefing, stamped
  delivered by it
  (`::a_contested_review_request_reaches_a_live_reviewer_only_as_its_briefing`),
  and a settlement the daemon died in is finished by the daemon that comes
  back (`::a_restart_finishes_a_settlement_the_daemon_died_in`).
- One pick per reviewer is the store's own rule too, and the winner reads
  off the picks with a tie to the first listed
  (`store.rs::a_reviewer_picks_once_and_the_picks_settle_a_winner`).
- The staffing rules hold at creation and on an edit
  (`store.rs::a_task_takes_several_authors_each_on_a_branch_of_its_own`,
  `::an_edit_replaces_the_whole_author_list`).

## Sources

`crates/ariadne-daemon/src/scheduler/tasks.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/http/landing.rs` (`pick_winner`),
`crates/ariadne-store/src/picks.rs`,
`crates/ariadne-store/src/defaults.rs` (`AUTHOR_*`, `REVIEWER_*`,
`CHANGES_REQUESTED`, `REVIEWER_PICK`).
