#!/usr/bin/env bash
# Ariadne uninstaller: removes the daemon service, binaries and shell
# completions installed by scripts/install.sh (locations read from
# ~/.ariadne/install.env). Data (~/.ariadne: database, worktrees, logs) is
# kept unless --purge is given.
#
# Idempotent: safe to run when partially or not installed.
#
# Usage: scripts/uninstall.sh [--prefix DIR] [--purge]
#   --prefix DIR  binaries location if no install manifest exists (default: ~/.local/bin)
#   --purge       also delete ~/.ariadne (database, worktrees, run dirs, logs)
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local/bin}"
PURGE=0
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --purge) PURGE=1; shift ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

OS="$(uname -s)"
ARIADNE_HOME="${ARIADNE_HOME:-$HOME/.ariadne}"
MANIFEST="$ARIADNE_HOME/install.env"
PLIST_LABEL="dev.ariadne.daemon"

# Defaults, overridden by the manifest when present.
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
ARIADNE_PREFIX="$PREFIX"
ARIADNE_BASH_COMPLETION="$DATA_DIR/bash-completion/completions/ariadne"
ARIADNE_ZSH_COMPLETION="$DATA_DIR/zsh/site-functions/_ariadne"
ARIADNE_PLIST="$HOME/Library/LaunchAgents/$PLIST_LABEL.plist"
ARIADNE_UNIT="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/ariadned.service"
if [ -f "$MANIFEST" ]; then
    # shellcheck disable=SC1090
    . "$MANIFEST"
fi

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }

strip_block() {
    local file="$1"
    [ -f "$file" ] || return 0
    awk '/^# >>> ariadne >>>/{skip=1} skip==0{print} /^# <<< ariadne <<</{skip=0}' \
        "$file" > "$file.ariadne-tmp"
    mv "$file.ariadne-tmp" "$file"
}

# --- daemon service ----------------------------------------------------------
say "stopping and removing the daemon service"
case "$OS" in
    Darwin)
        launchctl bootout "gui/$(id -u)/$PLIST_LABEL" 2>/dev/null || true
        rm -f "$ARIADNE_PLIST"
        ;;
    Linux)
        systemctl --user disable --now ariadned.service 2>/dev/null || true
        rm -f "$ARIADNE_UNIT"
        systemctl --user daemon-reload 2>/dev/null || true
        ;;
esac
# A manually started daemon, if any.
if [ -f "$ARIADNE_HOME/ariadned.pid" ]; then
    kill "$(cat "$ARIADNE_HOME/ariadned.pid")" 2>/dev/null || true
fi

# --- binaries -------------------------------------------------------------------
say "removing binaries from $ARIADNE_PREFIX"
rm -f "$ARIADNE_PREFIX/ariadne" "$ARIADNE_PREFIX/ariadned"

# --- completions ------------------------------------------------------------------
say "removing shell completions"
rm -f "$ARIADNE_BASH_COMPLETION" "$ARIADNE_ZSH_COMPLETION"
strip_block "$HOME/.bashrc"
strip_block "${ZDOTDIR:-$HOME}/.zshrc"

# --- data ------------------------------------------------------------------------
rm -f "$MANIFEST"
if [ "$PURGE" = 1 ]; then
    say "purging $ARIADNE_HOME (database, worktrees, run dirs, logs)"
    rm -rf "$ARIADNE_HOME"
else
    say "kept $ARIADNE_HOME (database, worktrees, logs) — re-run with --purge to delete"
fi

say "ariadne uninstalled"
