---
id: repositories-branches-and-worktrees
status: current
updated: 2026-09-06
areas: [store, daemon]
commits: [b6c6b9d2, 2bca45a6, 305ee064, 481a405d, a69b953f, 87fa62cf, a4d7da95]
tests:
  - crates/ariadne-daemon/tests/repositories.rs
  - crates/ariadne-daemon/tests/goal_repositories.rs
  - crates/ariadne-daemon/tests/managers.rs
  - crates/ariadne-daemon/tests/task_branches.rs
  - crates/ariadne-store/tests/store.rs
---

# Repositories, branches and worktrees

Where work happens on disk: the checkouts Ariadne is pointed at, the branch a
task is given, and the worktree each agent stands in.

## Scope

In: registering a repository, its base branch and description, the merge
strategy field, task branch naming, the worktree per seat, worktree cleanup,
and the watch on a task branch's head.

Out: how a task ends in it (005), and what an
agent is briefed with in its worktree (006).

## Behavior

1. A repository is registered once and referenced by every goal that works in
   it. It carries a path, a base branch and an optional description, and
   nothing else. How work *ends* in it is not a repository field: that is the
   task's own ending (005).
2. The base branch defaults to the branch the checkout is on at registration.
3. A path and branch pair is unique: the same one cannot be registered twice.
   A path or branch the daemon cannot use is refused at creation. A checkout
   with no commits is usable: its base branch is the unborn one HEAD names.
4. A repository a goal references cannot be deleted.
5. Editing the base branch changes what *new* tasks branch from, and nothing
   about tasks already under way.
6. A task branch is named after the task's title — its slug plus a short tail
   of its id, as in `fix-the-landing-briefing-real-fetch-r9jr7c`. Branch names
   carry no `ariadne/` prefix.
7. An author gets a writable worktree of its own, on its task branch, cut
   from the base branch of the task's repository. When that base has no
   commits the task branch is cut orphan, the author's first commit is the
   repository's first, and the task's diff is read against the empty tree
   because there is no merge base to read it against. It is read against the
   empty tree once it has landed too: that commit is the one the repository
   starts from, so it has no first parent either. Cutting orphan needs
   git 2.42, which is the floor `ariadne doctor` warns below (014).
8. A reviewer gets a **detached, read-only** worktree pinned to the branch
   under review, and it is refreshed between rounds so each round reads the
   commits that round added. A branch with no commits on it has nothing to
   pin at, and spawning a reviewer there says so.
9. An orchestrator works in the repository's primary checkout, not a worktree
   of its own: it is the first repository of its goal.
10. Worktrees are removed when the work that owned them ends; whether finished
    and cancelled work keeps its worktree for inspection is configuration.
11. The daemon watches each task branch's head and announces a move on the
    event stream, so clients see a commit without polling. The watch is
    established for the worktrees found at startup and goes when the worktree
    does; a failed task stops being followed.

## Acceptance criteria

- A repository round-trips through CRUD, emitting one fat event per write
  (`repositories.rs::crud_round_trip_emits_a_fat_event_per_write`).
- The base branch defaults to the checked-out branch
  (`repositories.rs::base_branch_defaults_to_the_current_branch`), a path or
  branch that cannot be used is refused
  (`::create_refuses_a_path_or_branch_it_cannot_use`), a checkout with no
  commits registers on its unborn branch
  (`::a_repository_with_no_commits_can_be_registered`), and the same pair
  cannot be registered twice (`::the_same_path_and_branch_cannot_be_registered_twice`).
- A task branches from the repository its goal references
  (`goal_repositories.rs::a_task_branches_from_the_repository_its_goal_references`),
  and editing the base branch moves only what new tasks branch from
  (`::editing_the_base_branch_moves_what_new_tasks_branch_from`).
- A repository in use cannot be deleted
  (`goal_repositories.rs::a_repository_in_use_cannot_be_deleted`,
  `store.rs::a_repository_a_goal_holds_cannot_be_deleted`).
- A branch is named after the task's title
  (`store.rs::task_branch_is_named_after_the_title`).
- Worktrees are created, verified and removed, and a reviewer's is refreshed
  between rounds (`managers.rs::git_worktree_lifecycle_and_merge_verification`,
  `::reviewer_worktree_refresh_between_rounds`); a base branch with no commits
  gives an orphan worktree that has nothing to review until it commits, and
  then diffs, lands, and diffs again as a landed task
  (`::a_worktree_is_cut_from_a_base_branch_with_no_commits`).
- A commit on a task branch reaches the stream
  (`task_branches.rs::a_commit_on_the_task_branch_reaches_the_stream`), the
  startup sweep follows the worktrees it finds
  (`::the_startup_sweep_follows_the_worktrees_it_finds`), a failed task stops
  being followed (`::a_failed_task_stops_being_followed`), and the watch goes
  with the worktree (`::the_watch_goes_with_the_worktree`).

## Sources

`crates/ariadne-daemon/src/gitwt.rs`, `crates/ariadne-daemon/src/branch.rs`,
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-store/src/repositories.rs`.
