#!/usr/bin/env bash
# Run on any OS: bash scripts/tests/github-settings.sh. A stand-in `gh` keeps a
# fake repository in a temporary directory; no network or credential is used.
set -Eeuo pipefail

REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT="$REPO_DIR/scripts/github-settings.sh"
ROOT="$(mktemp -d)"
trap 'rm -rf "$ROOT"' EXIT
mkdir -p "$ROOT/bin"

# The stand-in: state is $STATE/repo.json and $STATE/rulesets.json (an array).
cat >"$ROOT/bin/gh" <<'GH'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = api ] && shift
method=GET input=""
while [ "$#" -gt 0 ]; do
    case "$1" in
    --method) method="$2"; shift 2 ;;
    --input) input="$2"; shift 2 ;;
    *) path="$1"; shift ;;
    esac
done
printf '%s %s\n' "$method" "$path" >>"$STATE/calls"
repo="$STATE/repo.json" sets="$STATE/rulesets.json"
case "$method $path" in
"GET repos/o/r") cat "$repo" ;;
"PATCH repos/o/r") jq -s '.[0] * .[1]' "$repo" "$input" >"$repo.new" && mv "$repo.new" "$repo" ;;
"GET repos/o/r/rulesets") jq '[.[] | {id, name, target}]' "$sets" ;;
"GET repos/o/r/rulesets/"*) jq --argjson id "${path##*/}" '.[] | select(.id == $id)' "$sets" ;;
"PUT repos/o/r/rulesets/"*)
    jq --argjson id "${path##*/}" --slurpfile n "$input" 'map(if .id == $id then $n[0] + {id: $id} else . end)' "$sets" >"$sets.new" && mv "$sets.new" "$sets" ;;
"POST repos/o/r/rulesets") jq --slurpfile n "$input" '. + [$n[0] + {id: 99}]' "$sets" >"$sets.new" && mv "$sets.new" "$sets" ;;
*) echo "unexpected: $method $path" >&2; exit 1 ;;
esac
GH
chmod +x "$ROOT/bin/gh"
export PATH="$ROOT/bin:$PATH" STATE="$ROOT/state"

seed() {
    rm -rf "$STATE"
    mkdir -p "$STATE"
    cat >"$STATE/repo.json" <<'JSON'
{"description": "keep me", "has_wiki": true, "allow_auto_merge": true,
 "allow_squash_merge": true, "allow_merge_commit": true, "allow_rebase_merge": true,
 "squash_merge_commit_title": "COMMIT_OR_PR_TITLE", "delete_branch_on_merge": false}
JSON
    cat >"$STATE/rulesets.json" <<'JSON'
[{"id": 1, "name": "main", "target": "branch", "enforcement": "active", "bypass_actors": [],
  "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
  "rules": [{"type": "deletion"},
            {"type": "required_status_checks", "parameters": {"required_status_checks": []}}]},
 {"id": 2, "name": "other", "target": "tag", "enforcement": "active", "bypass_actors": [], "conditions": {}, "rules": []}]
JSON
}

fail() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }

# verify reports drift, exits nonzero and writes nothing.
seed
before="$(cat "$STATE/repo.json" "$STATE/rulesets.json")"
if out="$("$SCRIPT" verify --repo o/r 2>&1)"; then fail "verify accepted drifted settings"; fi
grep -q 'delete_branch_on_merge' <<<"$out" || fail "verify did not name the repository drift"
grep -q 'required_status_checks' <<<"$out" || fail "verify did not name the ruleset drift"
[ "$before" = "$(cat "$STATE/repo.json" "$STATE/rulesets.json")" ] || fail "verify changed settings"
grep -qv '^GET ' "$STATE/calls" && fail "verify made a write call"

# apply writes the managed fields and leaves the rest alone.
"$SCRIPT" apply --repo o/r >/dev/null
"$SCRIPT" verify --repo o/r >/dev/null || fail "verify rejected applied settings"
[ "$(jq -r .description "$STATE/repo.json")" = "keep me" ] || fail "apply changed description"
[ "$(jq -r .allow_auto_merge "$STATE/repo.json")" = true ] || fail "apply changed an unmanaged field"
[ "$(jq -r '.[] | select(.id == 2) | .name' "$STATE/rulesets.json")" = other ] || fail "apply touched another ruleset"
[ "$(jq length "$STATE/rulesets.json")" = 2 ] || fail "apply duplicated the ruleset"

# apply creates the ruleset when it is missing.
seed
jq 'map(select(.id != 1))' "$STATE/rulesets.json" >"$STATE/n" && mv "$STATE/n" "$STATE/rulesets.json"
"$SCRIPT" apply --repo o/r >/dev/null
"$SCRIPT" verify --repo o/r >/dev/null || fail "verify rejected a created ruleset"

# the managed ruleset has no bypass and no PR or status-check rule.
jq -e '.bypass_actors == [] and ([.rules[].type] | sort) == ["deletion","non_fast_forward","required_linear_history"]' \
    "$REPO_DIR/.github/settings/main-ruleset.json" >/dev/null || fail "ruleset rules differ from the policy"

printf 'PASS: github-settings\n'
