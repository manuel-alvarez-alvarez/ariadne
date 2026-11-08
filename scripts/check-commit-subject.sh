#!/usr/bin/env bash
# Validate one authored commit subject, also suitable for PR titles.
set -euo pipefail
[ "$#" = 1 ] || exit 1
subject="$1"
[[ "$subject" != *$'\n'* ]] || exit 1
types='feat|fix|perf|revert|docs|refactor|test|chore|style|build|ci'
scopes='api|bench|cli|client|codex|console|core|daemon|docs|doctor|forge|install|mcp|opencode|prompts|scripts|specs|stats|store|ui|workflows'
pattern="^(${types})(\\((${scopes})\\))?!?: .+"

if printf '%s' "$subject" | grep -Eq "$pattern"; then
    exit 0
fi

{
    printf 'commit subject must be "type(scope): subject" or "type: subject".\n'
    printf 'type is one of: %s\n' "$types"
    printf 'scope, when present, is one of: %s\n' "$scopes"
    printf 'got: %s\n' "$subject"
} >&2
exit 1
