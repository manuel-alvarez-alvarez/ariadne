#!/usr/bin/env bash
# Reproduction for RELEASE-001: scripts/uninstall.sh (and scripts/install.sh)
# source ~/.ariadne/install.env as bash, not as data. Five things, in order:
#   0. a manifest with ONLY well-formed KEY="VALUE" assignments (no injected
#      command at all) still gets an unrelated directory deleted — proving a
#      non-executing parser alone would not close the deletion half of this.
#   1. a manifest holding one extra line runs that line as the invoking user
#      during `uninstall.sh --dry-run`, which still reports "nothing was
#      changed".
#   2. the same, on a real (non-dry-run) run, plus the forged ARIADNE_APP
#      path is deleted with rm -rf.
#   3. a forged PURGE=1 wipes the whole $ARIADNE_HOME on a plain
#      `uninstall.sh`, with no --purge flag given at all.
#   4. nothing outside the throwaway $WORK changed while any of the above ran.
#
# Every child process below runs under `env -i` plus an explicit, complete
# set of home/config/data/log variables pointed inside a throwaway $WORK —
# nothing from the caller's real environment (ZDOTDIR, XDG_DATA_HOME,
# XDG_CONFIG_HOME, TMPDIR, or anything else) reaches install.sh/uninstall.sh.
# An earlier version of this script forgot ZDOTDIR and XDG_DATA_HOME, which
# — on a caller whose shell exports either — let uninstall.sh's
# ui_strip_block/GNOME-entry removal reach the caller's real dotfiles. The
# "ambient sentinel" files below exist specifically to catch a regression of
# that kind: they sit at the paths the caller's *real* ZDOTDIR/XDG_DATA_HOME
# would resolve to if this script's isolation ever broke again, and every
# assertion is a hard failure (set -e plus explicit checks), not a printed
# note.
set -Eeuo pipefail

REPO_DIR="$(cd "$(dirname "$0")/../../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/ariadne-release-001.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK"/{repo/scripts,bin,home/.ariadne,home/Documents}
cp "$REPO_DIR/scripts/"{install.sh,uninstall.sh,lib.sh} "$WORK/repo/scripts/"
chmod +x "$WORK/repo/scripts/"*.sh

# Stand-ins so the uninstaller never reaches a real launchd/systemd.
for tool in launchctl systemctl; do
    cat > "$WORK/bin/$tool" <<'EOS'
#!/usr/bin/env bash
exit 0
EOS
    chmod +x "$WORK/bin/$tool"
done

# A stand-in for "anything the real user cares about" that sits nowhere near
# install.sh's legitimate install footprint — proving the forged ARIADNE_APP
# path is honored verbatim, not just a path under $PREFIX or /Applications.
echo "do not delete me" > "$WORK/home/Documents/sentinel.txt"

# --- the ambient sentinels -----------------------------------------------
# These simulate whatever the *real* caller's environment happens to export,
# entirely outside $WORK/home. They are never passed to the child process
# (see run_isolated below); if any of them changes, this script's isolation
# has a hole, exactly like the one an earlier version had.
AMBIENT="$WORK/ambient-outside-the-fixture"
mkdir -p "$AMBIENT/zdotdir" "$AMBIENT/xdg-data/applications"
cat > "$AMBIENT/zdotdir/.zshrc" <<'EOF'
# >>> ariadne >>>
# a real completion block, as ariadne completions install would write it
# <<< ariadne <<<
echo "my own .zshrc, untouched by any of this"
EOF
AMBIENT_ZSHRC_BEFORE="$(cat "$AMBIENT/zdotdir/.zshrc")"
echo "a real GNOME entry, unrelated to this run" > "$AMBIENT/xdg-data/applications/dev.ariadne.ui.desktop"
AMBIENT_ENTRY_BEFORE="$(cat "$AMBIENT/xdg-data/applications/dev.ariadne.ui.desktop")"
# Exported in *this* shell only, to prove the child never inherits them
# (env -i strips them regardless of whether a caller's real shell sets the
# equivalent to something else entirely).
export ZDOTDIR="$AMBIENT/zdotdir"
export XDG_DATA_HOME="$AMBIENT/xdg-data"

# A manifest shaped like install.sh's own output (scripts/install.sh:704-719),
# with one line an attacker added — not install.sh. Nothing here requires
# install.sh to have ever run: this is what a forged ~/.ariadne/install.env
# looks like, however it got there.
cat > "$WORK/home/.ariadne/install.env" <<EOF
# Written by scripts/install.sh — read by scripts/uninstall.sh.
ARIADNE_PREFIX="$WORK/home/.local/bin"
ARIADNE_BASH_COMPLETION="$WORK/home/.local/share/bash-completion/completions/ariadne"
ARIADNE_ZSH_COMPLETION="$WORK/home/.local/share/zsh/site-functions/_ariadne"
ARIADNE_PLIST="$WORK/home/Library/LaunchAgents/dev.ariadne.daemon.plist"
ARIADNE_UNIT="$WORK/home/.config/systemd/user/ariadned.service"
touch "$WORK/PWNED"
ARIADNE_APP="$WORK/home/Documents"
EOF

echo "== forged ~/.ariadne/install.env =="
cat "$WORK/home/.ariadne/install.env"
echo

# Every variable ui_locations (scripts/lib.sh:87-103) or ui_strip_block can
# resolve a path from — HOME, ARIADNE_HOME, ZDOTDIR, XDG_DATA_HOME,
# XDG_CONFIG_HOME — is set explicitly, inside $WORK. `env -i` drops
# everything else (including whatever TMPDIR, LANG or ZDOTDIR the real
# caller's shell happens to export) so nothing but this fixed list can reach
# the scripts under test.
run_isolated() {
    env -i \
        PATH="$WORK/bin:/usr/bin:/bin" \
        HOME="$WORK/home" \
        ARIADNE_HOME="$WORK/home/.ariadne" \
        ZDOTDIR="$WORK/home" \
        XDG_DATA_HOME="$WORK/home/.local/share" \
        XDG_CONFIG_HOME="$WORK/home/.config" \
        TMPDIR="$WORK/tmp" \
        NO_COLOR=1 \
        bash "$@"
}
mkdir -p "$WORK/tmp"

echo "== 0. deletion does not need code execution: a pure KEY=\"VALUE\" manifest is enough =="
# Shows that parsing install.env as plain data (the fix for the code-execution
# half of RELEASE-001) does NOT, on its own, close the deletion half: this
# manifest contains no executable line at all, only assignments a data-only
# parser would accept just as readily as `. "$ARIADNE_MANIFEST"` does, naming
# an "Unrelated.app" that has nothing to do with Ariadne. See report.md's
# RELEASE-001 remediation for why the fix needs a second, independent check
# on *which paths an uninstall is allowed to remove*, not just a parser swap.
mkdir -p "$WORK/home3/.ariadne" "$WORK/home3/Applications/Unrelated.app"
echo "someone else's application" > "$WORK/home3/Applications/Unrelated.app/marker.txt"
cat > "$WORK/home3/.ariadne/install.env" <<EOF
# Written by scripts/install.sh — read by scripts/uninstall.sh. Every line
# below is a plain assignment; a non-executing KEY="VALUE" parser reads this
# file exactly as uninstall.sh's ". \$ARIADNE_MANIFEST" does today.
ARIADNE_PREFIX="$WORK/home3/.local/bin"
ARIADNE_APP="$WORK/home3/Applications/Unrelated.app"
EOF
env -i \
    PATH="$WORK/bin:/usr/bin:/bin" \
    HOME="$WORK/home3" \
    ARIADNE_HOME="$WORK/home3/.ariadne" \
    ZDOTDIR="$WORK/home3" \
    XDG_DATA_HOME="$WORK/home3/.local/share" \
    XDG_CONFIG_HOME="$WORK/home3/.config" \
    TMPDIR="$WORK/tmp" \
    NO_COLOR=1 \
    bash "$WORK/repo/scripts/uninstall.sh"
echo
[ ! -e "$WORK/home3/Applications/Unrelated.app" ] \
    || { echo "FAIL: expected Unrelated.app to have been deleted even though its manifest held no injected command."; exit 1; }
echo "OBSERVED: Unrelated.app was deleted by a manifest containing only well-formed assignments —"
echo "          a parser that refuses to execute the file would still have read ARIADNE_APP and"
echo "          handed it, unchecked, to rm -rf."
echo

echo "== 1. uninstall.sh --dry-run (user is only previewing, nothing should change) =="
run_isolated "$WORK/repo/scripts/uninstall.sh" --dry-run
echo

[ -f "$WORK/PWNED" ] \
    || { echo "FAIL: expected $WORK/PWNED to exist — the injected line did not run during --dry-run as claimed."; exit 1; }
echo "OBSERVED: $WORK/PWNED exists — the injected line ran during --dry-run, which claims \"nothing was changed\"."
rm -f "$WORK/PWNED"

echo
echo "== 2. uninstall.sh (the real run) =="
run_isolated "$WORK/repo/scripts/uninstall.sh"
echo

[ -f "$WORK/PWNED" ] \
    || { echo "FAIL: expected $WORK/PWNED to exist after the real run."; exit 1; }
echo "OBSERVED: $WORK/PWNED exists — the injected line ran again."

[ ! -d "$WORK/home/Documents" ] \
    || { echo "FAIL: expected $WORK/home/Documents (the forged ARIADNE_APP) to have been deleted."; exit 1; }
echo "OBSERVED: $WORK/home/Documents (the forged ARIADNE_APP, standing in for an arbitrary path) was deleted."

echo
echo "== 3. a forged manifest can also force --purge without the flag =="
# PURGE is set from the CLI flags (scripts/uninstall.sh:41-50) *before*
# ui_locations/the manifest source at scripts/uninstall.sh:69 — but sourcing
# runs after that assignment, in the same shell, so a forged manifest can
# simply reassign PURGE=1 (or ARIADNE_HOME itself) and scripts/uninstall.sh:161-165
# ("if [ "$PURGE" = 1 ]; then rm -rf "$ARIADNE_HOME"; fi") then wipes the
# whole data directory — database, worktrees, run dir, logs — on a plain
# `uninstall.sh` with no --purge on the command line at all.
mkdir -p "$WORK/home2/.ariadne"
echo "a database, a worktree, whatever ~/.ariadne actually holds" > "$WORK/home2/.ariadne/sentinel-data.txt"
cat > "$WORK/home2/.ariadne/install.env" <<EOF
# Written by scripts/install.sh — read by scripts/uninstall.sh.
ARIADNE_PREFIX="$WORK/home2/.local/bin"
PURGE=1
EOF
env -i \
    PATH="$WORK/bin:/usr/bin:/bin" \
    HOME="$WORK/home2" \
    ARIADNE_HOME="$WORK/home2/.ariadne" \
    ZDOTDIR="$WORK/home2" \
    XDG_DATA_HOME="$WORK/home2/.local/share" \
    XDG_CONFIG_HOME="$WORK/home2/.config" \
    TMPDIR="$WORK/tmp" \
    NO_COLOR=1 \
    bash "$WORK/repo/scripts/uninstall.sh" # no --purge flag
echo
[ ! -e "$WORK/home2/.ariadne" ] \
    || { echo "FAIL: expected \$ARIADNE_HOME ($WORK/home2/.ariadne) to have been wiped by the forged PURGE=1, with no --purge flag given."; exit 1; }
echo "OBSERVED: $WORK/home2/.ariadne was deleted by a plain 'uninstall.sh' invocation — the forged"
echo "          manifest's PURGE=1 reached scripts/uninstall.sh:161-165 even though --purge was never"
echo "          passed on the command line."

echo
echo "== 4. isolation check: did anything outside \$WORK change? =="
[ "$(cat "$AMBIENT/zdotdir/.zshrc")" = "$AMBIENT_ZSHRC_BEFORE" ] \
    || { echo "FAIL: the ambient ZDOTDIR sentinel .zshrc changed — isolation leaked."; exit 1; }
echo "OK: ambient ZDOTDIR/.zshrc is byte-for-byte unchanged."
[ "$(cat "$AMBIENT/xdg-data/applications/dev.ariadne.ui.desktop")" = "$AMBIENT_ENTRY_BEFORE" ] \
    || { echo "FAIL: the ambient XDG_DATA_HOME sentinel desktop entry changed — isolation leaked."; exit 1; }
echo "OK: ambient XDG_DATA_HOME desktop entry is byte-for-byte unchanged."

echo
echo "PASS: RELEASE-001 reproduced (code execution, arbitrary deletion, and a forced --purge), fixture isolation held."
