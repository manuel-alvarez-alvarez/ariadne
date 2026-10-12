#!/usr/bin/env bash
# Reproduction for RELEASE-001: scripts/uninstall.sh (and scripts/install.sh)
# source ~/.ariadne/install.env as bash, not as data. A manifest holding one
# extra line runs that line as the invoking user, and a forged ARIADNE_APP
# path is deleted with rm -rf, whether or not --dry-run was given.
#
# Self-contained: builds a throwaway HOME under mktemp, copies the real
# install.sh/uninstall.sh/lib.sh into it unmodified, and never touches the
# caller's own HOME or a real daemon/service. Run from anywhere:
#
#   bash security/audit/2026-10-12/08-release-and-install/repro-RELEASE-001.sh
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

echo "== 1. uninstall.sh --dry-run (user is only previewing, nothing should change) =="
PATH="$WORK/bin:$PATH" HOME="$WORK/home" ARIADNE_HOME="$WORK/home/.ariadne" \
    bash "$WORK/repo/scripts/uninstall.sh" --dry-run
echo
if [ -f "$WORK/PWNED" ]; then
    echo "OBSERVED: $WORK/PWNED exists — the injected line ran during --dry-run."
else
    echo "NOT REPRODUCED: --dry-run did not execute the injected line."
fi
rm -f "$WORK/PWNED"

echo
echo "== 2. uninstall.sh (the real run) =="
PATH="$WORK/bin:$PATH" HOME="$WORK/home" ARIADNE_HOME="$WORK/home/.ariadne" \
    bash "$WORK/repo/scripts/uninstall.sh"
echo
if [ -f "$WORK/PWNED" ]; then
    echo "OBSERVED: $WORK/PWNED exists — the injected line ran again."
else
    echo "NOT REPRODUCED: the injected line did not run."
fi
if [ -d "$WORK/home/Documents" ]; then
    echo "NOT REPRODUCED: $WORK/home/Documents (the forged ARIADNE_APP) is still present."
else
    echo "OBSERVED: $WORK/home/Documents (the forged ARIADNE_APP, standing in for an arbitrary path) was deleted."
fi
