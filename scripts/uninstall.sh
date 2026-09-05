#!/usr/bin/env bash
# Ariadne uninstaller: removes the daemon service, binaries, shell completions
# and the "Ariadne Desktop" app - and its GNOME entry and icon, on Linux -
# installed by scripts/install.sh (locations read from ~/.ariadne/install.env).
# Data (~/.ariadne: database, worktrees, logs) is kept unless --purge is given.
#
# Idempotent: safe to run when partially or not installed.
#
# Output is a numbered step list; noisy subcommands (launchctl, systemctl) go
# to $TMPDIR/ariadne-uninstall.log — never into ~/.ariadne, which --purge
# deletes — and are only shown when a step fails.
#
# The options are in usage() below, which is what --help prints; they are not
# repeated here so the two cannot drift apart.
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
  --yes, -y     accepted for symmetry with install.sh; nothing here asks
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

# The same locations install.sh writes, as the defaults; the manifest of the
# install actually being undone then overrides whichever of them it records.
ui_locations
LOG_FILE="${TMPDIR:-/tmp}/ariadne-uninstall.log"
ARIADNE_PREFIX="$PREFIX"
# Only ever set by the manifest: an install that skipped the desktop app - and
# any manifest written before it existed - leaves nothing to remove.
ARIADNE_APP=""
# Only ever set by the manifest: the icon's path carries its size, so unlike
# the entry itself (a fixed path from ui_locations) it cannot be guessed.
ARIADNE_DESKTOP_ICON=""
MANIFEST_FOUND=0
if [ -f "$ARIADNE_MANIFEST" ]; then
    MANIFEST_FOUND=1
    # shellcheck disable=SC1090
    . "$ARIADNE_MANIFEST"
fi

SERVICE_DESC="$(ui_service_desc "none on $OS")"

# --- the plan ------------------------------------------------------------------
# One plan_add per step, in execution order; the step count adapts to the flags.
plan_add "Stopping and removing the daemon service ($SERVICE_DESC)"
plan_add "Removing binaries from $(ui_tilde "$ARIADNE_PREFIX")"
[ -n "$ARIADNE_APP" ] && plan_add "Removing $(ui_tilde "$ARIADNE_APP")"
[ "$OS" = Linux ] && plan_add "Removing the GNOME entry"
plan_add "Removing shell completions"
plan_add "Removing the install manifest"
[ "$PURGE" = 1 ] && plan_add "Deleting $(ui_tilde "$ARIADNE_HOME") (database, worktrees, run dirs, logs)"
ui_start

ui_header "Ariadne uninstaller" \
    "manifest  $(ui_tilde "$ARIADNE_MANIFEST")$([ "$MANIFEST_FOUND" = 1 ] || printf ' (absent - using defaults)')" \
    "data      $([ "$PURGE" = 1 ] && printf 'deleted (--purge)' || printf 'kept')" \
    "log       $(ui_tilde "$LOG_FILE")"

if [ "$UI_DRY_RUN" = 1 ]; then
    plan_print
    exit 0
fi

ui_log_init "$LOG_FILE"

# --- daemon service ----------------------------------------------------------
step_begin
ui_stop_daemon unregister
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

# --- desktop app ------------------------------------------------------------------
APP_REMOVED=0
if [ -n "$ARIADNE_APP" ]; then
    step_begin
    if [ -e "$ARIADNE_APP" ]; then
        rm -rf "$ARIADNE_APP"
        APP_REMOVED=1
        step_ok
    else
        step_skip "nothing to remove"
    fi
fi

# --- gnome desktop entry (Linux only) ----------------------------------------------
GNOME_ENTRY_REMOVED=0
if [ "$OS" = Linux ]; then
    step_begin
    if [ -e "$ARIADNE_DESKTOP_ENTRY" ] || { [ -n "$ARIADNE_DESKTOP_ICON" ] && [ -e "$ARIADNE_DESKTOP_ICON" ]; }; then
        GNOME_ENTRY_REMOVED=1
    fi
    rm -f "$ARIADNE_DESKTOP_ENTRY"
    [ -n "$ARIADNE_DESKTOP_ICON" ] && rm -f "$ARIADNE_DESKTOP_ICON"
    if [ "$GNOME_ENTRY_REMOVED" = 1 ]; then
        step_ok
    else
        step_skip "nothing to remove"
    fi
fi

# --- completions ------------------------------------------------------------------
step_begin
rm -f "$ARIADNE_BASH_COMPLETION" "$ARIADNE_ZSH_COMPLETION"
ui_strip_block "$ARIADNE_BASHRC"
ui_strip_block "$ARIADNE_ZSHRC"
step_ok

# --- manifest ---------------------------------------------------------------------
step_begin
rm -f "$ARIADNE_MANIFEST"
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
[ "$APP_REMOVED" = 1 ] && ui_field "desktop app" "removed ($(ui_tilde "$ARIADNE_APP"))"
[ "$GNOME_ENTRY_REMOVED" = 1 ] && ui_field "gnome entry" "removed"
ui_field "completions" "removed (rc blocks stripped, new shells only)"
if [ "$PURGE" = 1 ]; then
    ui_field "data" "$(ui_tilde "$ARIADNE_HOME") deleted"
else
    ui_field "data" "$(ui_tilde "$ARIADNE_HOME") kept - re-run with --purge to delete"
fi
ui_field "log" "$(ui_tilde "$LOG_FILE")"
printf '\n'
