#!/bin/sh
# One interleaved round: the base revision's test files, then the branch's.
# Usage: round.sh BASE REP, from the worktree root of the committed branch.
# BASE is the base revision, such as `git merge-base HEAD main`.
set -eu
base=$1
rep=$2
it=crates/ariadne-daemon/tests/it
git diff --quiet HEAD -- "$it" || { echo "commit $it first" >&2; exit 1; }
for f in $(git diff --name-only "$base" HEAD -- "$it"); do
    if git cat-file -e "$base:$f" 2>/dev/null; then
        git show "$base:$f" > "$f"
    else
        rm "$f"
    fi
done
python3 bench/build-test-latency/fake-forge/run.py interleaved before "$rep" "$base"
git checkout HEAD -- "$it"
python3 bench/build-test-latency/fake-forge/run.py interleaved after "$rep" "$base"
