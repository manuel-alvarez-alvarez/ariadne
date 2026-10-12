# Reproductions

Both patches here are temporary test instrumentation only: apply, run the
named test, then `git checkout --` the touched file to revert. Neither was
left applied to the application source, and neither is part of the
application's normal test suite.

- `forge-003-review-mark-spoof.patch` — apply to
  `crates/ariadne-daemon/src/forge/news.rs`, then run:
  ```
  cargo nextest run -p ariadne-daemon \
    -E 'test(forge_003_an_attackers_own_comment_is_attributed_to_the_ariadne_review)'
  ```
  Proves an attacker's own comment is attributed to "the Ariadne review"
  in the agent's news feed, with no login check anywhere in that path.
  See report.md, finding FORGE-003.

- `forge-004-head-repo-no-validation.patch` — apply to
  `crates/ariadne-daemon/src/gitwt.rs`, then run:
  ```
  cargo nextest run -p ariadne-daemon \
    -E 'test(fetch_head_of_accepts_any_string_as_from_with_no_host_or_scheme_check)'
  ```
  Proves `GitManager::fetch_head_of` — the function
  `launcher.rs::review_worktree` calls with a forge-reported `head_repo` —
  applies no host or scheme validation at all, using two local, throwaway
  git repositories as a stand-in fork, never a network host. See
  report.md, finding FORGE-004.

Apply with `git apply security/audit/2026-10-12/03-forge-ingress/repro/<name>.patch`
from the repository root; revert with
`git checkout -- <the file the patch touched>`.

FORGE-001's reproduction is not a patch: it cites an existing, already-committed
test, `crates/ariadne-daemon/tests/it/pull_request_babysit.rs::asking_keeps_a_request_of_mine_by_hand_until_the_merge`,
run as-is. See report.md for the exact command and output.
