# Reproductions

The two patches here are temporary test instrumentation. Neither is part of
the application's test suite, and neither is ever applied to a tracked
worktree.

## The published workflow: `run-patches.sh`

Run it from anywhere, under a time limit of ten minutes or less:

```sh
security/audit/2026-10-12/03-forge-ingress/repro/run-patches.sh
```

What it does:

1. Resolves the two patch paths from its own directory, before any clone or
   checkout. The examined commit does not hold these files, so they are
   read from here.
2. Creates a new work directory and a new log directory with `mktemp -d`.
3. Clones the repository into the work directory, checks out
   `a17283486d5fe89591edfff08832712936d73030`, verifies `HEAD`, and applies
   both patches in the clone.
4. Runs the two tests with a nextest configuration that ends any test after
   five minutes.
5. Writes the test log to the log directory, outside any worktree, prints
   its path, and keeps it.
6. Removes the work directory — only that directory — on success, failure
   or interruption.

It exits with the test run's own status. A replay at this revision printed
`2 tests run: 2 passed`, exited 0 after 48 seconds, and left no work
directory behind.

- `forge-003-review-mark-spoof.patch` touches
  `crates/ariadne-daemon/src/forge/news.rs`. It shows that an attacker's
  own comment is attributed to "the Ariadne review" in the agent's news.
  See report.md, FORGE-003.
- `forge-004-head-repo-no-validation.patch` touches
  `crates/ariadne-daemon/src/gitwt.rs`. It shows that
  `GitManager::fetch_head_of`, which `launcher.rs::review_worktree` calls
  with a forge-reported `head_repo`, applies no host or scheme check. It
  uses two local throwaway repositories as a stand-in fork, never a network
  host. See report.md, FORGE-004.

## Evidence that needs no patch

- FORGE-001 cites an existing committed test, run as-is:
  `crates/ariadne-daemon/tests/it/pull_request_babysit.rs::asking_keeps_a_request_of_mine_by_hand_until_the_merge`.
- FORGE-005 cites an existing committed test, run as-is:
  `crates/ariadne-daemon/tests/it/tunnel.rs::a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once`.
- FORGE-007's git branch-name check uses no application code. It runs in
  any fresh, empty directory (for example one made with `mktemp -d`). The
  exact transcript is in report.md.

See report.md for each command and its output.
