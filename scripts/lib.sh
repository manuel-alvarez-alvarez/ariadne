#!/usr/bin/env bash
# Shared output framework for scripts/install.sh and scripts/uninstall.sh.
#
# Both scripts are a list of numbered steps: the plan is built first (so the
# step count adapts to the flags), then each step runs and reports ✓ / ↷ / ✗.
# Noisy subcommands (cargo, npm, launchctl, systemctl) are captured to a log
# file and only shown when something fails.
#
# Adding a step is two lines: a `plan_add "title"` where the plan is built and
# a `step_begin` / `step_ok` pair where the work happens, in the same order.
#
# Sourced, never executed. Everything here is bash 3.2 (macOS) compatible.

# The flags and style variables defined here are read by the sourcing script,
# which shellcheck cannot see from this file alone.
# shellcheck disable=SC2034

# --- shared flags (see ui_common_flag) ----------------------------------------
UI_VERBOSE=0   # stream subcommand output instead of capturing it
UI_QUIET=0     # errors and the final summary only
UI_DRY_RUN=0   # print the plan, touch nothing
UI_YES=0       # non-interactive: skip anything that would ask

# --- internal state -----------------------------------------------------------
UI_PLAN=()       # planned step titles, in order
UI_TOTAL=0
UI_STEP=0
UI_STEP_OPEN=0   # a step is running (its line may still be pending)
UI_PENDING=0     # an unterminated line is on screen, waiting to be overwritten
UI_LOG=""        # log file for captured output; empty until ui_log_init
UI_FAILED=0

# Consume one flag shared by both scripts. Returns 1 when $1 is not ours, so
# callers can fall through to their own options.
ui_common_flag() {
    case "$1" in
        --verbose) UI_VERBOSE=1; UI_QUIET=0 ;;
        --quiet) UI_QUIET=1; UI_VERBOSE=0 ;;
        --dry-run) UI_DRY_RUN=1 ;;
        --yes|-y) UI_YES=1 ;;
        *) return 1 ;;
    esac
    return 0
}

# Colors, symbols and cursor control, all off unless stdout is a terminal that
# has not asked for plain output (NO_COLOR, TERM=dumb).
ui_init() {
    if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != "dumb" ]; then
        UI_FANCY=1
    else
        UI_FANCY=0
    fi

    if [ "$UI_FANCY" = 1 ]; then
        UI_B=$'\033[1m'; UI_D=$'\033[2m'; UI_R=$'\033[0m'
        UI_GREEN=$'\033[32m'; UI_YELLOW=$'\033[33m'; UI_RED=$'\033[31m'
        UI_CYAN=$'\033[36m'
        UI_CLR=$'\r\033[2K'
        UI_OK='✓'; UI_SKIP='↷'; UI_FAIL='✗'; UI_RUN='·'
        UI_ARROW='→'; UI_BULLET='↳'
    else
        UI_B=''; UI_D=''; UI_R=''
        UI_GREEN=''; UI_YELLOW=''; UI_RED=''; UI_CYAN=''
        UI_CLR=''
        UI_OK='OK  '; UI_SKIP='SKIP'; UI_FAIL='FAIL'; UI_RUN='..  '
        UI_ARROW='->'; UI_BULLET='-'
    fi
}

# Shorten $HOME to ~ so paths stay readable in the summary.
ui_tilde() {
    case "$1" in
        "$HOME") printf '~' ;;
        "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;;
        *) printf '%s' "$1" ;;
    esac
}

# --- what an install puts where -------------------------------------------------
# install.sh writes these paths into the manifest and uninstall.sh reads them
# back, so the two scripts have to agree on them exactly. They are resolved
# here once instead of being spelled out on both sides.
#
# Call before reading a manifest: an uninstall lets the manifest of the
# install it is undoing override every one of them.
ui_locations() {
    local _data_dir
    OS="$(uname -s)"
    ARIADNE_HOME="${ARIADNE_HOME:-$HOME/.ariadne}"
    ARIADNE_MANIFEST="$ARIADNE_HOME/install.env"
    ARIADNE_PLIST_LABEL="dev.ariadne.daemon"
    ARIADNE_PLIST="$HOME/Library/LaunchAgents/$ARIADNE_PLIST_LABEL.plist"
    ARIADNE_UNIT="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/ariadned.service"
    _data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
    ARIADNE_BASH_COMPLETION="$_data_dir/bash-completion/completions/ariadne"
    ARIADNE_ZSH_COMPLETION="$_data_dir/zsh/site-functions/_ariadne"
    ARIADNE_DESKTOP_ID="dev.ariadne.ui"
    ARIADNE_DESKTOP_ENTRY="$_data_dir/applications/$ARIADNE_DESKTOP_ID.desktop"
    ARIADNE_ICON_BASE="$_data_dir/icons/hicolor"
    ARIADNE_BASHRC="$HOME/.bashrc"
    ARIADNE_ZSHRC="${ZDOTDIR:-$HOME}/.zshrc"
}

# `install.env` is data, never shell input. These are the only keys the
# installer writes and the uninstaller may read.
ui_manifest_key_allowed() {
    case "$1" in
        ARIADNE_PREFIX|ARIADNE_BASH_COMPLETION|ARIADNE_ZSH_COMPLETION|ARIADNE_PLIST|ARIADNE_UNIT|ARIADNE_APP|ARIADNE_DESKTOP_ENTRY|ARIADNE_DESKTOP_ICON)
            return 0
            ;;
        *) return 1 ;;
    esac
}

# Keep manifest values to one inert `KEY=value` line. The reader does not
# evaluate these characters, but refusing them also prevents a new manifest
# from becoming executable input to an older installer or uninstaller.
ui_manifest_value_safe() {
    case "$1" in
        *'"'*|*'$'*|*'`'*|*$'\n'*|*$'\r'*) return 1 ;;
        *) return 0 ;;
    esac
}

# Read an installer manifest as data. The values are left in MANIFEST_* so a
# caller chooses which recorded locations it should use. Existing manifests
# quoted every value; accept that safe legacy form while all new files use raw
# `KEY=value` lines.
ui_read_install_manifest() {
    local _path="$1" _line _key _value _seen
    MANIFEST_PREFIX=""
    MANIFEST_BASH_COMPLETION=""
    MANIFEST_ZSH_COMPLETION=""
    MANIFEST_PLIST=""
    MANIFEST_UNIT=""
    MANIFEST_APP=""
    MANIFEST_DESKTOP_ENTRY=""
    MANIFEST_DESKTOP_ICON=""
    _seen=" "

    [ -f "$_path" ] || return 1
    while IFS= read -r _line || [ -n "$_line" ]; do
        case "$_line" in
            ''|'#'*) continue ;;
            *=*)
                _key="${_line%%=*}"
                _value="${_line#*=}"
                ;;
            *) ui_die "invalid line in install manifest $_path" ;;
        esac
        ui_manifest_key_allowed "$_key" \
            || ui_die "unexpected key in install manifest: $_key"
        case "$_seen" in
            *" $_key "*) ui_die "duplicate key in install manifest: $_key" ;;
        esac
        _seen="$_seen$_key "
        case "$_value" in
            \"*\") _value="${_value#\"}"; _value="${_value%\"}" ;;
        esac
        ui_manifest_value_safe "$_value" \
            || ui_die "unsafe value in install manifest: $_key"
        case "$_key" in
            ARIADNE_PREFIX) MANIFEST_PREFIX="$_value" ;;
            ARIADNE_BASH_COMPLETION) MANIFEST_BASH_COMPLETION="$_value" ;;
            ARIADNE_ZSH_COMPLETION) MANIFEST_ZSH_COMPLETION="$_value" ;;
            ARIADNE_PLIST) MANIFEST_PLIST="$_value" ;;
            ARIADNE_UNIT) MANIFEST_UNIT="$_value" ;;
            ARIADNE_APP) MANIFEST_APP="$_value" ;;
            ARIADNE_DESKTOP_ENTRY) MANIFEST_DESKTOP_ENTRY="$_value" ;;
            ARIADNE_DESKTOP_ICON) MANIFEST_DESKTOP_ICON="$_value" ;;
        esac
    done < "$_path"
}

# Refuse a value before the caller replaces an existing manifest with it.
ui_validate_install_manifest_value() {
    local _key="$1" _value="$2"
    ui_manifest_key_allowed "$_key" \
        || ui_die "cannot write unexpected install manifest key: $_key"
    ui_manifest_value_safe "$_value" \
        || ui_die "cannot write install manifest: $_key contains shell syntax"
}

# Append one verified manifest value. `install.sh` owns the file itself so it
# can put its comment at the top; this function only writes values.
ui_write_install_manifest_value() {
    local _path="$1" _key="$2" _value="$3"
    ui_validate_install_manifest_value "$_key" "$_value"
    printf '%s=%s\n' "$_key" "$_value" >> "$_path"
}

# How the daemon is registered on this OS, for step titles and summaries.
# $1 is what to say on an OS Ariadne registers no service for.
ui_service_desc() {
    case "$OS" in
        Darwin) printf 'launchd %s' "$ARIADNE_PLIST_LABEL" ;;
        Linux) printf 'systemd --user ariadned.service' ;;
        *) printf '%s' "$1" ;;
    esac
}

# Remove a previously added "# >>> ariadne >>> ... # <<< ariadne <<<" block.
ui_strip_block() {
    local file="$1"
    [ -f "$file" ] || return 0
    awk '/^# >>> ariadne >>>/{skip=1} skip==0{print} /^# <<< ariadne <<</{skip=0}' \
        "$file" > "$file.ariadne-tmp"
    mv "$file.ariadne-tmp" "$file"
}

# Poll $@ once a second until it fails, or until ~30s have passed. Used where
# something needs a moment to go away - launchd dropping a service, a killed
# process exiting - and the install has to go on either way: a stop that
# never finishes is not a reason to fail it.
ui_wait_until_gone() {
    local _i
    for _i in $(seq 1 30); do
        "$@" > /dev/null 2>&1 || return 0
        sleep 1
    done
    return 0
}

# Stop whatever daemon is running: the registered service, plus one started by
# hand from its pid file. With `unregister`, the service is also disabled and
# its unit file deleted - what the uninstaller wants, where the installer only
# wants the binaries free to be replaced.
#
# `launchctl bootout` and a plain `kill` both return before the process is
# actually gone, so each is followed by a wait - a `bootstrap` or a start
# right behind either one can otherwise land on a label or a pid that has not
# finished clearing.
ui_stop_daemon() {
    local mode="${1:-}" pid pid_extra _pid_extra
    case "$OS" in
        Darwin)
            run_logged launchctl bootout "gui/$(id -u)/$ARIADNE_PLIST_LABEL" || true
            if [ "$mode" = unregister ]; then rm -f "$ARIADNE_PLIST"; fi
            ui_wait_until_gone launchctl print "gui/$(id -u)/$ARIADNE_PLIST_LABEL"
            ;;
        Linux)
            if [ "$mode" = unregister ]; then
                run_logged systemctl --user disable --now ariadned.service || true
                rm -f "$ARIADNE_UNIT"
                run_logged systemctl --user daemon-reload || true
            else
                run_logged systemctl --user stop ariadned.service || true
            fi
            ;;
    esac
    if [ -f "$ARIADNE_HOME/ariadned.pid" ]; then
        pid=""
        pid_extra=0
        {
            IFS= read -r pid || true
            if IFS= read -r _pid_extra || [ -n "${_pid_extra:-}" ]; then
                pid_extra=1
            fi
        } < "$ARIADNE_HOME/ariadned.pid"
        if [ "$pid_extra" = 0 ] && ui_pid_is_safe "$pid"; then
            pid="$UI_SAFE_PID"
            kill "$pid" 2>/dev/null || true
            ui_wait_until_gone kill -0 "$pid"
        else
            ui_warn "ignoring invalid daemon pidfile at $ARIADNE_HOME/ariadned.pid"
        fi
    fi
    return 0
}

# `kill -1` means every process the user may signal, not PID -1. Keep only
# decimal process ids above one, and normalise leading zeroes before invoking
# the shell's `kill` builtin.
ui_pid_is_safe() {
    local _pid="$1" _zeros
    case "$_pid" in
        ''|*[!0-9]*) return 1 ;;
    esac
    _zeros="${_pid%%[!0]*}"
    UI_SAFE_PID="${_pid#"$_zeros"}"
    case "$UI_SAFE_PID" in
        ''|0|1) return 1 ;;
    esac
    return 0
}

# --- header, plan and summary --------------------------------------------------

ui_header() {
    [ "$UI_QUIET" = 1 ] && return 0
    printf '\n%s%s%s\n' "$UI_B" "$1" "$UI_R"
    shift
    for _line in "$@"; do
        printf '%s  %s%s\n' "$UI_D" "$_line" "$UI_R"
    done
    printf '\n'
    return 0
}

# A label/value pair, aligned; used by the closing summary.
ui_field() {
    printf '  %s%-13s%s %s\n' "$UI_D" "$1" "$UI_R" "$2"
}

ui_warn() {
    printf '  %s%s%s %s\n' "$UI_YELLOW" "$UI_BULLET" "$UI_R" "$1"
}

ui_error() {
    printf '  %s%s%s %s\n' "$UI_RED" "$UI_FAIL" "$UI_R" "$1" >&2
}

plan_add() {
    UI_PLAN[${#UI_PLAN[@]}]="$1"
}

# Freeze the plan; must be called once every plan_add has run.
ui_start() {
    UI_TOTAL=${#UI_PLAN[@]}
}

# --dry-run output: exactly the steps that would run, and nothing else.
plan_print() {
    local i=1
    printf '%sWould run %d step(s):%s\n' "$UI_B" "$UI_TOTAL" "$UI_R"
    while [ "$i" -le "$UI_TOTAL" ]; do
        printf '  %s[%d/%d]%s %s\n' "$UI_D" "$i" "$UI_TOTAL" "$UI_R" \
            "${UI_PLAN[$((i - 1))]}"
        i=$((i + 1))
    done
    printf '\n%sDry run - nothing was changed.%s\n' "$UI_D" "$UI_R"
}

# --- steps ----------------------------------------------------------------------

# Render "[3/8] <marker> <title> (<note>)".
_ui_line() {
    local marker="$1" color="$2" note="$3"
    printf '%s[%d/%d]%s %s%s%s %s' \
        "$UI_D" "$UI_STEP" "$UI_TOTAL" "$UI_R" \
        "$color" "$marker" "$UI_R" "${UI_PLAN[$((UI_STEP - 1))]}"
    [ -n "$note" ] && printf ' %s(%s)%s' "$UI_D" "$note" "$UI_R"
    printf '\n'
}

# Start the next planned step. Its title comes from the plan, so --dry-run and
# the real run can never disagree.
step_begin() {
    UI_STEP=$((UI_STEP + 1))
    UI_STEP_OPEN=1
    [ "$UI_QUIET" = 1 ] && return 0
    if [ "$UI_VERBOSE" = 1 ]; then
        # Subcommand output follows, so the line has to be terminated.
        _ui_line "$UI_RUN" "$UI_D" ""
    elif [ "$UI_FANCY" = 1 ]; then
        # Held open and overwritten by the result marker.
        printf '%s[%d/%d]%s %s%s%s %s' \
            "$UI_D" "$UI_STEP" "$UI_TOTAL" "$UI_R" \
            "$UI_D" "$UI_RUN" "$UI_R" "${UI_PLAN[$((UI_STEP - 1))]}"
        UI_PENDING=1
    fi
    # Plain non-TTY output prints the whole line once the result is known.
    return 0
}

_ui_end() {
    local marker="$1" color="$2" note="$3"
    UI_STEP_OPEN=0
    if [ "$UI_PENDING" = 1 ]; then
        printf '%s' "$UI_CLR"
        UI_PENDING=0
    fi
    _ui_line "$marker" "$color" "$note"
}

step_ok() {
    [ "$UI_QUIET" = 1 ] && { UI_STEP_OPEN=0; return 0; }
    _ui_end "$UI_OK" "$UI_GREEN" "${1:-}"
    return 0
}

step_skip() {
    [ "$UI_QUIET" = 1 ] && { UI_STEP_OPEN=0; return 0; }
    _ui_end "$UI_SKIP" "$UI_YELLOW" "${1:-}"
    return 0
}

# Failures are printed even under --quiet.
step_fail() {
    UI_FAILED=1
    _ui_end "$UI_FAIL" "$UI_RED" "${1:-}"
    return 0
}

# --- running subcommands ---------------------------------------------------------

# Create the log file (truncated) for captured output.
ui_log_init() {
    UI_LOG="$1"
    mkdir -p "$(dirname "$UI_LOG")"
    : > "$UI_LOG"
}

# Run a command, capturing its output to the log unless --verbose.
run_logged() {
    if [ "$UI_VERBOSE" = 1 ]; then
        "$@"
        return $?
    fi
    if [ -n "$UI_LOG" ]; then
        printf '\n$ %s\n' "$*" >> "$UI_LOG"
        "$@" >> "$UI_LOG" 2>&1
        return $?
    fi
    "$@" > /dev/null 2>&1
    return $?
}

# Run a command that talks to the user: never captured, never hidden.
run_interactive() {
    if [ "$UI_PENDING" = 1 ]; then
        printf '%s' "$UI_CLR"
        UI_PENDING=0
    fi
    "$@"
}

# Print the tail of the log after a failure, with its path.
ui_log_tail() {
    [ -n "$UI_LOG" ] && [ -f "$UI_LOG" ] || return 0
    [ "$UI_VERBOSE" = 1 ] && return 0
    printf '\n%slast 30 lines of %s:%s\n' "$UI_D" "$(ui_tilde "$UI_LOG")" "$UI_R" >&2
    tail -n 30 "$UI_LOG" >&2
    printf '\n%sfull log: %s%s\n' "$UI_D" "$(ui_tilde "$UI_LOG")" "$UI_R" >&2
    return 0
}

# Fail the current step, show the log and stop.
ui_die() {
    if [ "$UI_STEP_OPEN" = 1 ]; then
        step_fail "${1:-failed}"
    else
        UI_FAILED=1
        ui_error "${1:-failed}"
    fi
    ui_log_tail
    exit 1
}

# Installed as the ERR trap so an unguarded command cannot fail silently.
ui_on_err() {
    [ "$UI_FAILED" = 1 ] && exit "${1:-1}"
    ui_die "step failed"
}
