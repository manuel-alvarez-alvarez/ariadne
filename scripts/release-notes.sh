#!/usr/bin/env bash
# Builds the generated part of a GitHub Release's body: a short install
# section, a table of the release's assets, and a compare link to the
# previous tag when one is known. Wrapped in two HTML comment markers so a
# re-run on its own output replaces only that part and never stacks a
# second header.
#
#   scripts/release-notes.sh TAG BODY_FILE CHECKSUMS_FILE...
#
# TAG is the release tag, e.g. v0.3.0. BODY_FILE holds the release's current
# body: the release-please changelog entry on a first run, or a previous
# run's own output on a re-run. Each CHECKSUMS_FILE is a
# checksums-<target>.txt staged by .github/workflows/release.yml, one
# `<digest>  <name>` line per asset. The new body is printed to stdout.
set -Eeuo pipefail

START='<!-- ariadne-release-notes:start -->'
END='<!-- ariadne-release-notes:end -->'

TAG="${1:?name the release tag}"
BODY_FILE="${2:?name the file holding the current release body}"
shift 2
[ "$#" -gt 0 ] || { echo "name at least one checksums file" >&2; exit 2; }
[ -f "$BODY_FILE" ] || { echo "$BODY_FILE: no such file" >&2; exit 2; }

# A body edited in the GitHub web UI, or returned by some API calls, carries
# \r\n line ends; a bare $0 == end in awk and a line-anchored match never see
# the marker under one, so the split below would run off the end and drop
# every release-please note. Work from a \n-only copy instead of the file
# the caller gave us.
BODY="$(mktemp)"
trap 'rm -f "$BODY"' EXIT
tr -d '\r' < "$BODY_FILE" > "$BODY"

# owner/repo the asset links and the compare link point at, read off the
# origin remote the same way scripts/install.sh resolves a release
# repository (handles the https, ssh and scp-like forms of the URL), with
# the Actions-provided GITHUB_REPOSITORY as a fallback for a checkout with
# no remote configured.
resolve_repo() {
    local url owner name
    url="$(git remote get-url origin 2>/dev/null)" || url=""
    if [ -z "$url" ]; then
        [ -n "${GITHUB_REPOSITORY:-}" ] \
            || { echo "no 'origin' remote and no GITHUB_REPOSITORY to take owner/repo from" >&2; exit 2; }
        printf '%s\n' "$GITHUB_REPOSITORY"
        return
    fi
    url="${url%.git}"
    url="${url%/}"
    name="${url##*/}"
    owner="${url%/*}"
    owner="${owner##*[:/]}"
    [ -n "$owner" ] && [ -n "$name" ] && [ "$owner" != "$url" ] \
        || { echo "could not read owner/repo out of the origin remote ($url)" >&2; exit 2; }
    printf '%s/%s\n' "$owner" "$name"
}
REPO="$(resolve_repo)"

# The previous tag, read from the body first: a "Full Changelog" compare
# line, ours from an earlier run, stays good on a re-run with no need to ask
# git again. Falls back to git for a first run, where the body carries none.
COMPARE_LINE="$(grep -oE 'compare/[^[:space:])]+' "$BODY" 2>/dev/null | head -1)" || true
if [ -n "$COMPARE_LINE" ]; then
    rest="${COMPARE_LINE#compare/}"
    PREV_TAG="${rest%%...*}"
else
    PREV_TAG="$(git describe --tags --abbrev=0 "$TAG^" 2>/dev/null)" || PREV_TAG=""
fi

# The release-please notes: whatever follows our own end marker, or the
# whole input when this is the first run and no marker exists yet. A body
# with one marker but not the other is not ours to guess at.
HAS_START=0; grep -qF "$START" "$BODY" && HAS_START=1
HAS_END=0; grep -qF "$END" "$BODY" && HAS_END=1
if [ "$HAS_START" -ne "$HAS_END" ]; then
    echo "$BODY_FILE: has one of the ariadne-release-notes markers but not the other" >&2
    exit 2
fi
if [ "$HAS_START" -eq 1 ]; then
    NOTES="$(awk -v end="$END" 'found { print } $0 == end { found = 1 }' "$BODY")"
else
    NOTES="$(cat "$BODY")"
fi
# Drop the blank line the split leaves at the top.
NOTES="$(printf '%s\n' "$NOTES" | sed '/./,$!d')"

echo "$START"
echo "## Install"
echo
echo '```sh'
echo "scripts/install.sh --version $TAG      # CLI, daemon service, completions, desktop app"
echo "ariadne daemon start                   # unix socket at ~/.ariadne/ariadne.sock"
echo '```'
echo
echo "See the [install guide](https://github.com/$REPO/blob/main/docs/install.md) for connecting an agent."
echo
echo "## Assets"
echo
echo "| Asset | Target | SHA-256 |"
echo "| --- | --- | --- |"
for checksums in "$@"; do
    [ -f "$checksums" ] || { echo "$checksums: no such file" >&2; exit 2; }
    base="$(basename "$checksums")"
    target="${base#checksums-}"
    target="${target%.txt}"
    while read -r digest name; do
        [ -n "$name" ] || continue
        echo "| [$name](https://github.com/$REPO/releases/download/$TAG/$name) | $target | $digest |"
    done < "$checksums"
done
if [ -n "$PREV_TAG" ]; then
    echo
    echo "**Full Changelog**: https://github.com/$REPO/compare/$PREV_TAG...$TAG"
fi
echo "$END"
if [ -n "$NOTES" ]; then
    echo
    printf '%s\n' "$NOTES"
fi
