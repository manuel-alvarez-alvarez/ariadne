#!/usr/bin/env bash
# Ariadne uninstaller: removes the daemon service, binaries and shell
# completions installed by scripts/install.sh (locations read from
# ~/.ariadne/install.env). Data (~/.ariadne: database, worktrees, logs) is
# kept unless --purge is given.
#
# Idempotent: safe to run when partially or not installed.
#
# Output is a numbered step list; noisy subcommands (launchctl, systemctl) go
# to $TMPDIR/ariadne-uninstall.log — never into ~/.ariadne, which --purge
# deletes — and are only shown when a step fails.
#
# Usage: scripts/uninstall.sh [--prefix DIR] [--purge] [--verbose] [--quiet]
#                             [--dry-run] [--yes] [--help]
#   --prefix DIR  binaries location if no install manifest exists (default: ~/.local/bin)
#   --purge       also delete ~/.ariadne (database, worktrees, run dirs, logs)
#   --verbose     stream subcommand output instead of capturing it
#   --quiet       print errors and the final summary only
#   --dry-run     print the steps that would run, change nothing
#   --yes, -y     non-interactive: do not ask before --purge deletes data
#   --help, -h    show usage
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/lib.sh
. "$REPO_DIR/scripts/lib.sh"

usage() {
    cat <<'EOF'
Ariadne uninstaller - removes what scripts/install.sh installed.

Usage: scripts/uninstall.sh [options]

  --prefix DIR  binaries location if no install manifest exists
                (default: ~/.local/bin)
  --purge       also delete ~/.ariadne (database, worktrees, run dirs, logs)
  --verbose     stream subcommand output instead of capturing it
  --quiet       print errors and the final summary only
  --dry-run     print the steps that would run, change nothing
  --yes, -y     non-interactive: do not ask before --purge deletes data
  --help, -h    show this help

Environment: PREFIX, ARIADNE_HOME, NO_COLOR.
EOF
}

PREFIX="${PREFIX:-$HOME/.local/bin}"
PURGE=0
while [ $# -gt 0 ]; do
    if ui_common_flag "$1"; then shift; continue; fi
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --purge) PURGE=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; echo >&2; usage >&2; exit 2 ;;
    esac
done
ui_init
trap 'ui_on_err $?' ERR

OS="$(uname -s)"
ARIADNE_HOME="${ARIADNE_HOME:-$HOME/.ariadne}"
MANIFEST="$ARIADNE_HOME/install.env"
LOG_FILE="${TMPDIR:-/tmp}/ariadne-uninstall.log"
PLIST_LABEL="dev.ariadne.daemon"

# Defaults, overridden by the manifest when present.
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
ARIADNE_PREFIX="$PREFIX"
ARIADNE_BASH_COMPLETION="$DATA_DIR/bash-completion/completions/ariadne"
ARIADNE_ZSH_COMPLETION="$DATA_DIR/zsh/site-functions/_ariadne"
ARIADNE_PLIST="$HOME/Library/LaunchAgents/$PLIST_LABEL.plist"
ARIADNE_UNIT="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/ariadned.service"
MANIFEST_FOUND=0
if [ -f "$MANIFEST" ]; then
    MANIFEST_FOUND=1
    # shellcheck disable=SC1090
    . "$MANIFEST"
fi

case "$OS" in
    Darwin) SERVICE_DESC="launchd $PLIST_LABEL" ;;
    Linux) SERVICE_DESC="systemd --user ariadned.service" ;;
    *) SERVICE_DESC="none on $OS" ;;
esac

strip_block() {
    local file="$1"
    [ -f "$file" ] || return 0
    awk '/^# >>> ariadne >>>/{skip=1} skip==0{print} /^# <<< ariadne <<</{skip=0}' \
        "$file" > "$file.ariadne-tmp"
    mv "$file.ariadne-tmp" "$file"
}

# --- the plan ------------------------------------------------------------------
# One plan_add per step, in execution order; the step count adapts to the flags.
plan_add "Stopping and removing the daemon service ($SERVICE_DESC)"
plan_add "Removing binaries from $(ui_tilde "$ARIADNE_PREFIX")"
plan_add "Removing shell completions"
plan_add "Removing the install manifest"
[ "$PURGE" = 1 ] && plan_add "Deleting $(ui_tilde "$ARIADNE_HOME") (database, worktrees, run dirs, logs)"
ui_start

ui_header "Ariadne uninstaller" \
    "manifest  $(ui_tilde "$MANIFEST")$([ "$MANIFEST_FOUND" = 1 ] || printf ' (absent - using defaults)')" \
    "data      $([ "$PURGE" = 1 ] && printf 'deleted (--purge)' || printf 'kept')" \
    "log       $(ui_tilde "$LOG_FILE")"

if [ "$UI_DRY_RUN" = 1 ]; then
    plan_print
    exit 0
fi

# --purge deletes the database and every worktree; confirm unless told not to.
if [ "$PURGE" = 1 ] && [ "$UI_YES" = 0 ] && [ -t 0 ] && [ -d "$ARIADNE_HOME" ]; then
    printf '  %s%s%s delete %s and everything in it? [y/N] ' \
        "$UI_YELLOW" "$UI_BULLET" "$UI_R" "$(ui_tilde "$ARIADNE_HOME")"
    read -r reply || reply=""
    case "$reply" in
        y|Y|yes|YES) ;;
        *) printf '\n'; ui_error "aborted"; exit 1 ;;
    esac
    printf '\n'
fi

ui_log_init "$LOG_FILE"

# --- daemon service ----------------------------------------------------------
step_begin
case "$OS" in
    Darwin)
        run_logged launchctl bootout "gui/$(id -u)/$PLIST_LABEL" || true
        rm -f "$ARIADNE_PLIST"
        ;;
    Linux)
        run_logged systemctl --user disable --now ariadned.service || true
        rm -f "$ARIADNE_UNIT"
        run_logged systemctl --user daemon-reload || true
        ;;
esac
# A manually started daemon, if any.
if [ -f "$ARIADNE_HOME/ariadned.pid" ]; then
    kill "$(cat "$ARIADNE_HOME/ariadned.pid")" 2>/dev/null || true
fi
step_ok

# --- binaries -------------------------------------------------------------------
step_begin
BINARIES_FOUND=0
if [ -e "$ARIADNE_PREFIX/ariadne" ] || [ -e "$ARIADNE_PREFIX/ariadned" ]; then
    BINARIES_FOUND=1
fi
rm -f "$ARIADNE_PREFIX/ariadne" "$ARIADNE_PREFIX/ariadned"
if [ "$BINARIES_FOUND" = 1 ]; then
    step_ok
else
    step_skip "nothing to remove"
fi

# --- completions ------------------------------------------------------------------
step_begin
rm -f "$ARIADNE_BASH_COMPLETION" "$ARIADNE_ZSH_COMPLETION"
strip_block "$HOME/.bashrc"
strip_block "${ZDOTDIR:-$HOME}/.zshrc"
step_ok

# --- manifest ---------------------------------------------------------------------
step_begin
rm -f "$MANIFEST"
if [ "$MANIFEST_FOUND" = 1 ]; then
    step_ok
else
    step_skip "none was written"
fi

# --- data ------------------------------------------------------------------------
if [ "$PURGE" = 1 ]; then
    step_begin
    rm -rf "$ARIADNE_HOME"
    step_ok
fi

# --- summary ----------------------------------------------------------------------
printf '\n%sAriadne uninstalled.%s\n\n' "$UI_B$UI_GREEN" "$UI_R"
ui_field "service" "removed ($SERVICE_DESC)"
if [ "$BINARIES_FOUND" = 1 ]; then
    ui_field "binaries" "removed from $(ui_tilde "$ARIADNE_PREFIX")"
else
    ui_field "binaries" "none found in $(ui_tilde "$ARIADNE_PREFIX")"
fi
ui_field "completions" "removed (rc blocks stripped, new shells only)"
if [ "$PURGE" = 1 ]; then
    ui_field "data" "$(ui_tilde "$ARIADNE_HOME") deleted"
else
    ui_field "data" "$(ui_tilde "$ARIADNE_HOME") kept - re-run with --purge to delete"
fi
ui_field "log" "$(ui_tilde "$LOG_FILE")"
printf '\n'
