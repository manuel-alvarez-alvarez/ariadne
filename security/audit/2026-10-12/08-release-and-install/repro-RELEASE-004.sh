#!/usr/bin/env bash
# Reproduction for RELEASE-004: scripts/install.sh interpolates $PREFIX
# (a plain CLI flag / environment variable, not shell-evaluated) directly
# into the generated systemd unit and launchd plist, with no escaping for
# either file format. A $PREFIX containing a newline followed by text that
# happens to be a valid unit/plist directive is not caught by "nothing here
# is eval'd" — systemd and launchd parse the *file*, not a shell command, and
# the file's grammar is what gets broken out of.
#
# This extracts the exact heredoc install.sh writes today (by `sed` line
# range, not a hand copy, so a future edit to install.sh that moves the
# lines makes this script fail loudly instead of silently testing stale
# code) and replays it in a disposable directory with a crafted $PREFIX.
# Nothing here starts a real systemd/launchd job — the claim under test is
# "the generated file now contains an attacker-chosen second directive",
# checked by inspecting the file, not by running a service manager against
# it (seeing RELEASE-004 in report.md for why that is an explicit limitation,
# not an oversight).
set -Eeuo pipefail

REPO_DIR="$(cd "$(dirname "$0")/../../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/ariadne-release-004.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/out"

fail() { echo "FAIL: $1"; exit 1; }

# --- 1. the systemd unit (scripts/install.sh's Linux branch) -----------------
UNIT_START="$(grep -n "^            cat > \"\$ARIADNE_UNIT\" <<EOF\$" "$REPO_DIR/scripts/install.sh" | cut -d: -f1)"
[ -n "$UNIT_START" ] || fail "could not find the ARIADNE_UNIT heredoc in scripts/install.sh — line anchor moved, update this script"
UNIT_START=$((UNIT_START - 1)) # include the preceding mkdir -p line
UNIT_END="$(awk -v start="$UNIT_START" 'NR>=start && NR>start+1 && /^EOF$/ {print NR; exit}' "$REPO_DIR/scripts/install.sh")"
[ -n "$UNIT_END" ] || fail "could not find the closing EOF of the ARIADNE_UNIT heredoc"
sed -n "${UNIT_START},${UNIT_END}p" "$REPO_DIR/scripts/install.sh" > "$WORK/unit-snippet.sh"
echo "== extracted scripts/install.sh:${UNIT_START}-${UNIT_END} (unmodified) =="
cat "$WORK/unit-snippet.sh"
echo

# A $PREFIX with no "/" in the injected part, so mkdir -p (called elsewhere
# in the real install.sh, not in this snippet) would stay inside one
# directory component rather than splitting across the filesystem; here it
# only has to survive being written into a heredoc and read back.
MALICIOUS_PREFIX="$WORK/out/prefix
ExecStartPre=-/usr/bin/touch $WORK/out/UNIT-INJECTED
#"
ARIADNE_UNIT="$WORK/out/ariadned.service"
PREFIX="$MALICIOUS_PREFIX"
# shellcheck disable=SC1090,SC1091
. "$WORK/unit-snippet.sh"

echo "== generated $ARIADNE_UNIT =="
cat "$ARIADNE_UNIT"
echo

grep -qxF 'ExecStartPre=-/usr/bin/touch '"$WORK"'/out/UNIT-INJECTED' "$ARIADNE_UNIT" \
    || fail "expected ExecStartPre=... to appear as its own directive line in the generated unit"
echo "OBSERVED: a second, syntactically valid directive (ExecStartPre=...) was injected as its own line."
echo "          systemd.unit(5) runs every ExecStartPre before ExecStart; had this been a live"
echo "          'systemctl --user restart ariadned.service' (as install.sh itself runs at"
echo "          scripts/install.sh:677), that command would have run with the invoking user's"
echo "          privileges, every time the service starts."
echo

# --- 2. the launchd plist (scripts/install.sh's Darwin branch) ---------------
PLIST_START="$(grep -n "^            cat > \"\$ARIADNE_PLIST\" <<EOF\$" "$REPO_DIR/scripts/install.sh" | cut -d: -f1)"
[ -n "$PLIST_START" ] || fail "could not find the ARIADNE_PLIST heredoc in scripts/install.sh — line anchor moved, update this script"
PLIST_START=$((PLIST_START - 1))
PLIST_END="$(awk -v start="$PLIST_START" 'NR>=start && NR>start+1 && /^EOF$/ {print NR; exit}' "$REPO_DIR/scripts/install.sh")"
[ -n "$PLIST_END" ] || fail "could not find the closing EOF of the ARIADNE_PLIST heredoc"
sed -n "${PLIST_START},${PLIST_END}p" "$REPO_DIR/scripts/install.sh" > "$WORK/plist-snippet.sh"
echo "== extracted scripts/install.sh:${PLIST_START}-${PLIST_END} (unmodified) =="
cat "$WORK/plist-snippet.sh"
echo

# The payload closes the current <string>/<array> early, injects a new
# sibling <key>/<array> pair, then re-opens a <string>/<array> of its own so
# that the template's own trailing "/ariadned</string></array>" (appended
# right after $PREFIX, unconditionally) closes *that* pair instead of being
# left as dangling, invalid text — the result is well-formed XML, not just
# a parse error, which is the more convincing proof: a validating plist
# reader accepts it and hands back an attacker-named key.
MALICIOUS_PREFIX_PLIST='/tmp/x</string></array><key>INJECTED</key><string>yes</string><key>Ignored</key><array><string>X'
ARIADNE_PLIST="$WORK/out/dev.ariadne.daemon.plist"
# shellcheck disable=SC2034 # read by the sourced plist-snippet.sh heredoc
ARIADNE_PLIST_LABEL="dev.ariadne.daemon"
# shellcheck disable=SC2034
PREFIX="$MALICIOUS_PREFIX_PLIST"
# shellcheck disable=SC2034
ARIADNE_HOME="$WORK/out"
PATH="/usr/bin:/bin"
# shellcheck disable=SC1090,SC1091
. "$WORK/plist-snippet.sh"

echo "== generated $ARIADNE_PLIST =="
cat "$ARIADNE_PLIST"
echo

if command -v python3 > /dev/null 2>&1; then
    python3 - "$ARIADNE_PLIST" <<'PYEOF'
import plistlib, sys
with open(sys.argv[1], "rb") as f:
    data = plistlib.load(f)
assert "INJECTED" in data, f"expected an INJECTED key in the parsed plist, got keys: {list(data)}"
print(f"OBSERVED: the parsed plist now has an attacker-named top-level key: INJECTED={data['INJECTED']!r}")
print(f"          ProgramArguments is now: {data.get('ProgramArguments')!r} (not the single ariadned path install.sh intended)")
PYEOF
else
    grep -qF '<key>INJECTED</key>' "$ARIADNE_PLIST" \
        || fail "expected an injected <key>INJECTED</key> element in the generated plist"
    echo "OBSERVED: <key>INJECTED</key> appears in the generated plist (python3 unavailable for a full parse)."
fi

echo
echo "PASS: RELEASE-004's generation-time injection reproduced for both the systemd unit and the launchd plist."
echo "LIMITATION: no real systemd/launchd was run against either file (out of this audit's scope); the"
echo "            systemd claim rests on systemd.unit(5)'s documented grammar (Key=Value lines, #/; comments,"
echo "            multiple ExecStartPre= lines all run in order), not an executed service."
