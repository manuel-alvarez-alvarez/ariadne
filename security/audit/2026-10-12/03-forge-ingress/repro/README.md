# Reproductions

Both patches here are temporary test instrumentation only, and neither is
part of the application's normal test suite. **Apply them to a disposable
local clone of the examined commit, never to a tracked worktree** — the
task requires temporary copies for any source instrumentation, and this
directory's patches are kept precisely so that copy can be recreated and
deleted on demand rather than ever touching application files in place.

## Workflow (what was actually run to verify both patches)

```sh
# From anywhere outside this worktree:
rm -rf /tmp/forge-repro-a1728348
git clone -q <path-to-this-repository> /tmp/forge-repro-a1728348
cd /tmp/forge-repro-a1728348
git checkout -q a17283486d5fe89591edfff08832712936d73030

git apply /path/to/security/audit/2026-10-12/03-forge-ingress/repro/forge-003-review-mark-spoof.patch
git apply /path/to/security/audit/2026-10-12/03-forge-ingress/repro/forge-004-head-repo-no-validation.patch

cargo nextest run -p ariadne-daemon \
  -E 'test(forge_003_an_attackers_own_comment_is_attributed_to_the_ariadne_review) | test(fetch_head_of_accepts_any_string_as_from_with_no_host_or_scheme_check)'
# -> PASS, PASS; Summary: 2 tests run: 2 passed, 965 skipped

cd / && rm -rf /tmp/forge-repro-a1728348
```

Each patch can also be applied and run individually, the same way, inside
its own disposable clone. Neither patch was ever applied to, or reverted
from, this worktree's own tracked files.

- `forge-003-review-mark-spoof.patch` touches
  `crates/ariadne-daemon/src/forge/news.rs`. Proves an attacker's own
  comment is attributed to "the Ariadne review" in the agent's news feed,
  with no login check anywhere in that path. See report.md, finding
  FORGE-003.

- `forge-004-head-repo-no-validation.patch` touches
  `crates/ariadne-daemon/src/gitwt.rs`. Proves `GitManager::fetch_head_of`
  — the function `launcher.rs::review_worktree` calls with a forge-reported
  `head_repo` — applies no host or scheme validation at all, using two
  local, throwaway git repositories as a stand-in fork, never a network
  host. See report.md, finding FORGE-004.

FORGE-001's reproduction is not a patch: it cites an existing,
already-committed test,
`crates/ariadne-daemon/tests/it/pull_request_babysit.rs::asking_keeps_a_request_of_mine_by_hand_until_the_merge`,
run as-is (no instrumentation, so no disposable copy is needed — it is
already part of the examined commit). See report.md for the exact command
and output.

FORGE-007's git-ref-name check needs no copy of the application at all:
it demonstrates a fact about git itself, in its own empty, throwaway
repository (`/tmp/forge-branch-check`, created and deleted for the check,
holding no application code). See report.md for the exact transcript.
