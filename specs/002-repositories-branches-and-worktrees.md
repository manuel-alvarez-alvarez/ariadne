---
id: repositories-branches-and-worktrees
status: current
updated: 2026-10-07
areas: [store, daemon]
commits: [b6c6b9d2, 2bca45a6, 305ee064, 481a405d, a69b953f, 87fa62cf, a4d7da95]
tests:
  - crates/ariadne-daemon/src/branch.rs
  - crates/ariadne-daemon/tests/it/repositories.rs
  - crates/ariadne-daemon/tests/it/ai_permissions.rs
  - crates/ariadne-daemon/tests/it/goal_repositories.rs
  - crates/ariadne-daemon/tests/it/managers.rs
  - crates/ariadne-daemon/tests/it/task_branches.rs
  - crates/ariadne-store/tests/store.rs
---

# Repositories, branches and worktrees

Where work happens on disk: the checkouts Ariadne is pointed at, the branch a
task is given, and the worktree each agent stands in.

## Scope

In: registering a repository, its base branch, description, permission mode
and default workflow, task branch naming, the one worktree a task's columns
share, worktree cleanup, and the watch on a task branch's head.

Out: how a task ends in it (030), what an
agent is briefed with in its worktree (006), and the forge its remote is on
and the integration with it (025).

## Behavior

1. A repository is registered once and referenced by every goal that works in
   it. It carries a path, a base branch, an optional description, a permission
   mode and a default workflow. The default is `develop-review-merge`; a goal
   that leaves its workflow out takes the default of its first repository
   (030). Changing it changes no existing goal.
2. The base branch defaults to the branch the checkout is on at registration.
3. A path and branch pair is unique: the same one cannot be registered twice.
   A path or branch the daemon cannot use is refused at creation. A checkout
   with no commits is usable: its base branch is the unborn one HEAD names.
4. A repository a goal references cannot be deleted.
5. Editing the base branch changes what *new* tasks branch from, and nothing
   about tasks already under way.
6. A task branch is named after the task's title — its slug plus a short tail
   of its id, as in `fix-the-merging-briefing-real-fetch-r9jr7c`. Branch names
   carry no `ariadne/` prefix. Every column of the task works on that one
   branch (030).
7. A task gets one writable worktree, cut from the repository base on its own
   task branch, and every column's agent works in it in turn (030): the
   develop column commits there, the review column reads and runs the suite
   there, and the merge column lands from there. When the repository base
   has no commits the task branch is cut orphan, the first commit is the
   repository's first, and the task's diff is read against the empty tree
   because there is no merge base to read it against. It is read against the
   empty tree once it has landed too: that commit is the one the repository
   starts from, so it has no first parent either. Cutting orphan needs
   git 2.42, which is the floor `ariadne doctor` warns below (014).
8. A review session on a pull request (029) gets a **detached, read-only**
   worktree pinned at the request's head, and it is refreshed to the new tip
   on every push. The refresh discards whatever the session left in the
   tree, tracked edits and untracked files alike, and keeps ignored files; a
   leftover edit never stops the tree from moving. A failed git start is not
   an empty branch: the start error remains visible, and the scheduler
   retries it.
9. An orchestrator works in the repository's primary checkout, not a worktree
   of its own: it is the first repository of its goal.
10. Worktrees are removed when the work that owned them ends; whether finished
    and cancelled work keeps its worktree for inspection is configuration.
11. The daemon watches each task branch's head and announces a move on the
    event stream, so clients see a commit without polling. The watch is
    established for the worktrees found at startup and goes when the worktree
    does; a failed task stops being followed. All followed branches in one
    repository share one filesystem watch. After a ref update, the daemon
    recreates that watch, so refs deleted by git pack-refs remain neither
    watched nor open. A repository wake still resolves each followed branch
    once, and only a moved branch emits an event.
12. The permission mode — `auto`, `ask`, `learn` or `ai` — says how every ACP
    session that works in the repository answers its permission requests
    (021). It defaults to `auto`, is set when the repository is registered
    and changed by editing it, from the CLI and the desktop app alike, and
    an edit that does not name it keeps it. It is set nowhere else: not per
    task, not in the daemon's configuration. `ai` is refused while the AI
    permission model is off, on registration and on an edit alike, with 409
    `ai_disabled` (022): the mode asks the model, and a model that is off
    answers nothing.

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
- A repository is `auto` until told otherwise, takes a mode at registration
  and on an edit that names only the mode, and refuses a mode it does not
  know (`repositories.rs::a_repository_carries_its_permission_mode`,
  `store.rs::repository_crud_and_unique_path_branch`).
- `ai` is refused while the AI permission model is off and taken once it
  is on
  (`ai_permissions.rs::a_repository_takes_the_ai_mode_only_once_the_model_is_on`,
  `store.rs::a_repository_takes_the_ai_permission_mode`).
- A repository defaults new goals to `merge`, takes another default at
  registration or on an edit, and changes no existing goal
  (`repositories.rs::a_repository_defaults_new_goals_without_changing_existing_ones`,
  `store.rs::repository_crud_and_unique_path_branch`).
- A task branches from the repository its goal references
  (`goal_repositories.rs::a_task_branches_from_the_repository_its_goal_references`),
  and editing the base branch moves only what new tasks branch from
  (`::editing_the_base_branch_moves_what_new_tasks_branch_from`).
- A repository in use cannot be deleted
  (`goal_repositories.rs::a_repository_in_use_cannot_be_deleted`,
  `store.rs::a_repository_a_goal_holds_cannot_be_deleted`).
- A branch is named after the task's title
  (`store.rs::task_branch_is_named_after_the_title`).
- Worktrees are created, verified and removed, and a detached one is
  refreshed to the new tip over whatever was left in it
  (`managers.rs::git_worktree_lifecycle_and_merge_verification`,
  `::a_detached_worktree_is_refreshed_to_the_new_tip`); a base branch with no
  commits gives an orphan worktree that has nothing to review until it
  commits, and then diffs, lands, and diffs again as a landed task
  (`::a_worktree_is_cut_from_a_base_branch_with_no_commits`).
- A commit on a task branch reaches the stream
  (`task_branches.rs::a_commit_on_the_task_branch_reaches_the_stream`), the
  startup sweep follows the worktrees it finds
  (`::the_startup_sweep_follows_the_worktrees_it_finds`), a failed task stops
  being followed (`::a_failed_task_stops_being_followed`), and the watch goes
  with the worktree (`::the_watch_goes_with_the_worktree`).
- Twelve followed branches in a repository with 100 loose refs use a bounded
  number of descriptors
  (`branch.rs::twelve_followed_branches_share_one_repository_watch`), and
  packing those refs releases the descriptors for deleted files
  (`branch.rs::packing_refs_releases_deleted_ref_descriptors`).
- A repository watch that failed to arm can start on the next follow request
  (`branch.rs::a_failed_repository_watch_can_be_started_again`).

## Sources

`crates/ariadne-daemon/src/gitwt.rs`, `crates/ariadne-daemon/src/branch.rs`,
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-store/src/repositories.rs`.
