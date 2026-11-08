#!/usr/bin/env bash
# Validate one authored commit subject, also suitable for PR titles.
set -euo pipefail
[ "$#" = 1 ] || exit 1
subject="$1"
[[ "$subject" != *$'\n'* ]] || exit 1
types='feat|fix|perf|revert|docs|refactor|test|chore|style|build|ci'
scopes='api|bench|cli|client|codex|console|core|daemon|docs|doctor|forge|install|mcp|opencode|prompts|scripts|specs|stats|store|ui|workflows'
pattern="^(${types})(\\((${scopes})\\))?!?: .+"

# Dependabot uses deps for dependency maintenance, outside the area scopes.
dependency_pattern='^chore\(deps\)!?: .+'
if printf '%s' "$subject" | grep -Eq -e "$pattern" -e "$dependency_pattern"; then
    exit 0
fi

{
    printf 'commit subject must be "type(scope): subject" or "type: subject".\n'
    printf 'type is one of: %s\n' "$types"
    printf 'scope, when present, is one of: %s\n' "$scopes"
    printf 'chore also accepts the deps scope for dependency updates.\n'
    printf 'got: %s\n' "$subject"
} >&2
exit 1
