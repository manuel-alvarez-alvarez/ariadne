#!/usr/bin/env bash
# Runs the FORGE-003 and FORGE-004 reproduction tests against the examined
# commit, in a fresh clone under a new temporary directory. Never touches a
# tracked worktree. Run it under a limit of ten minutes or less.
set -euo pipefail

EXAMINED=a17283486d5fe89591edfff08832712936d73030

# Resolved before any clone or checkout: the patches are read from this
# directory, not from the examined commit, which does not hold them.
REPRO=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(git -C "$REPRO" rev-parse --show-toplevel)
PATCHES=("$REPRO/forge-003-review-mark-spoof.patch" "$REPRO/forge-004-head-repo-no-validation.patch")

WORK=$(mktemp -d "${TMPDIR:-/tmp}/forge-repro.XXXXXX")
LOGS=$(mktemp -d "${TMPDIR:-/tmp}/forge-repro-logs.XXXXXX")
# Removes only the directory this run created, on success, failure or interrupt.
trap 'cd /; rm -rf "$WORK"' EXIT
echo "logs: $LOGS"

git clone -q --no-hardlinks "$ROOT" "$WORK/repo"
git -C "$WORK/repo" checkout -q --detach "$EXAMINED"
test "$(git -C "$WORK/repo" rev-parse HEAD)" = "$EXAMINED"
git -C "$WORK/repo" apply "${PATCHES[@]}"

# Each test is ended after five minutes at most.
printf '[profile.default]\nslow-timeout = { period = "60s", terminate-after = 5 }\n' \
    > "$WORK/nextest.toml"

cd "$WORK/repo"
status=0
CARGO_TARGET_DIR="$WORK/target" cargo nextest run -p ariadne-daemon \
    --config-file "$WORK/nextest.toml" \
    -E 'test(forge_003_an_attackers_own_comment_is_attributed_to_the_ariadne_review) | test(fetch_head_of_accepts_any_string_as_from_with_no_host_or_scheme_check)' \
    > "$LOGS/patches.log" 2>&1 || status=$?
grep -E '^ *(PASS|FAIL|TIMEOUT)|Summary' "$LOGS/patches.log" || tail -n 40 "$LOGS/patches.log"
exit "$status"
