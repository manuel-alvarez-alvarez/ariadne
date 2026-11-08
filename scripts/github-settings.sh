#!/usr/bin/env bash
# Apply or verify the GitHub settings kept under .github/settings/.
#   scripts/github-settings.sh apply  [--repo OWNER/REPO]
#   scripts/github-settings.sh verify [--repo OWNER/REPO]
# `apply` writes only the fields named in repository.json and the one ruleset
# named in main-ruleset.json. `verify` only reads: it prints each difference
# and exits 1 when the live settings differ. Credentials stay with `gh`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
settings="$root/.github/settings"

usage() {
    printf 'usage: %s apply|verify [--repo OWNER/REPO]\n' "$0" >&2
    exit 2
}

[ "$#" -ge 1 ] || usage
mode="$1"
shift
repo=""
while [ "$#" -gt 0 ]; do
    case "$1" in
    --repo)
        [ "$#" -ge 2 ] || usage
        repo="$2"
        shift 2
        ;;
    *) usage ;;
    esac
done
case "$mode" in apply | verify) ;; *) usage ;; esac
[ -n "$repo" ] || repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"

ruleset_name="$(jq -r .name "$settings/main-ruleset.json")"

# The live ruleset id, or nothing when no ruleset has the managed name.
live_ruleset_id() {
    gh api "repos/$repo/rulesets" |
        jq -r --arg n "$ruleset_name" '[.[] | select(.name == $n and .target == "branch")][0].id // empty'
}

# Reduce a ruleset to the managed fields, in a stable order.
normalize_ruleset() {
    jq -S '{name, target, enforcement, bypass_actors, conditions, rules: (.rules | map({type} + (if .parameters then {parameters} else {} end)) | sort_by(.type))}'
}

if [ "$mode" = apply ]; then
    gh api --method PATCH "repos/$repo" --input "$settings/repository.json" >/dev/null
    id="$(live_ruleset_id)"
    if [ -n "$id" ]; then
        gh api --method PUT "repos/$repo/rulesets/$id" --input "$settings/main-ruleset.json" >/dev/null
    else
        gh api --method POST "repos/$repo/rulesets" --input "$settings/main-ruleset.json" >/dev/null
    fi
    printf 'applied GitHub settings to %s\n' "$repo"
    exit 0
fi

status=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

keys="$(jq -c 'keys' "$settings/repository.json")"
gh api "repos/$repo" | jq -S --argjson k "$keys" '. as $r | $k | map({key: ., value: $r[.]}) | from_entries' >"$tmp/repo.live"
jq -S . "$settings/repository.json" >"$tmp/repo.want"
if ! diff -u --label live --label wanted "$tmp/repo.live" "$tmp/repo.want"; then
    printf 'repository settings differ for %s\n' "$repo" >&2
    status=1
fi

id="$(live_ruleset_id)"
if [ -z "$id" ]; then
    printf 'ruleset "%s" is missing from %s\n' "$ruleset_name" "$repo" >&2
    exit 1
fi
gh api "repos/$repo/rulesets/$id" | normalize_ruleset >"$tmp/rules.live"
normalize_ruleset <"$settings/main-ruleset.json" >"$tmp/rules.want"
if ! diff -u --label live --label wanted "$tmp/rules.live" "$tmp/rules.want"; then
    printf 'ruleset "%s" differs for %s\n' "$ruleset_name" "$repo" >&2
    status=1
fi

[ "$status" = 0 ] && printf 'GitHub settings of %s match\n' "$repo"
exit "$status"
