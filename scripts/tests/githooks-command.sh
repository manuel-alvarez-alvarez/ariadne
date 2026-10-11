#!/usr/bin/env bash
# Stand-in for cargo, npm, npx and shellcheck in the githooks boundary tests.
# Logs its own invocation and exits 1 only when TEST_FAIL names it exactly.
set -euo pipefail
tool="$(basename "$0")"
line="$tool"
if [ $# -gt 0 ]; then
    line="$tool $*"
fi
printf '%s\n' "$line" >> "$TEST_ROOT/commands"
if [ "${TEST_FAIL:-}" = "$line" ]; then
    printf '%s: stand-in failure\n' "$tool" >&2
    exit 1
fi

if [ "${TEST_GIT_PROBE:-}" = 1 ] && [ "$tool" = cargo ]; then
    case "${1:-} ${2:-}" in
        'clippy '*|'nextest run') ;;
        *) exit 0 ;;
    esac
    git -C "$TEST_ROOT/foreign" update-ref refs/heads/githooks-probe HEAD
    git -C "$TEST_ROOT/foreign" read-tree --empty
    git -C "$TEST_ROOT/foreign" checkout -q --detach
fi
