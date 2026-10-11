#!/usr/bin/env bash
# Point git at the repository's .githooks/ directory. The setting is the
# repository's, shared by every worktree once one install has run it.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

current="$(git config --get core.hooksPath 2>/dev/null || true)"
if [ "$current" = ".githooks" ]; then
    printf 'core.hooksPath is already .githooks\n'
    exit 0
fi

git config core.hooksPath .githooks
printf 'set core.hooksPath to .githooks\n'
