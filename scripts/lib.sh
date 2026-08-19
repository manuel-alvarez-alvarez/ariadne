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

ui_note() {
    [ "$UI_QUIET" = 1 ] && return 0
    printf '      %s%s %s%s\n' "$UI_D" "$UI_BULLET" "$1" "$UI_R"
    return 0
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
