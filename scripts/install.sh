#!/usr/bin/env bash
# Ariadne installer: builds from source, installs the binaries, registers the
# daemon as a user service (launchd on macOS, systemd --user on Linux),
# installs bash/zsh completions and has the user trust Ariadne's Codex hooks.
#
# Idempotent: safe to re-run after upgrades or config changes; every step
# replaces what a previous run installed. What was installed where is
# recorded in ~/.ariadne/install.env, which uninstall.sh reads.
#
# Output is a numbered step list; noisy subcommands (cargo, launchctl,
# systemctl) go to ~/.ariadne/install.log and are only shown when a step fails.
#
# Usage: scripts/install.sh [--prefix DIR] [--no-service] [--no-completions]
#                           [--no-codex-hooks] [--verbose] [--quiet]
#                           [--dry-run] [--yes] [--help]
#   --prefix DIR       install binaries into DIR (default: ~/.local/bin)
#   --no-service       skip daemon service registration
#   --no-completions   skip shell completion installation
#   --no-codex-hooks   skip the Codex hook trust prompt
#   --verbose          stream subcommand output instead of capturing it
#   --quiet            print errors and the final summary only
#   --dry-run          print the steps that would run, change nothing
#   --yes, -y          non-interactive: skip anything that would ask
#   --help, -h         show usage
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/lib.sh
. "$REPO_DIR/scripts/lib.sh"

usage() {
    cat <<'EOF'
Ariadne installer - builds from source and installs ariadne + ariadned.

Usage: scripts/install.sh [options]

  --prefix DIR       install binaries into DIR (default: ~/.local/bin)
  --no-service       skip daemon service registration (launchd / systemd --user)
  --no-completions   skip shell completion installation
  --no-codex-hooks   skip the Codex hook trust prompt
  --verbose          stream subcommand output instead of capturing it
  --quiet            print errors and the final summary only
  --dry-run          print the steps that would run, change nothing
  --yes, -y          non-interactive: skip anything that would ask
  --help, -h         show this help

Environment: PREFIX, ARIADNE_HOME, NO_COLOR.
EOF
}

PREFIX="${PREFIX:-$HOME/.local/bin}"
WITH_SERVICE=1
WITH_COMPLETIONS=1
WITH_CODEX_HOOKS=1
while [ $# -gt 0 ]; do
    if ui_common_flag "$1"; then shift; continue; fi
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --no-service) WITH_SERVICE=0; shift ;;
        --no-completions) WITH_COMPLETIONS=0; shift ;;
        --no-codex-hooks) WITH_CODEX_HOOKS=0; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; echo >&2; usage >&2; exit 2 ;;
    esac
done
ui_init
trap 'ui_on_err $?' ERR

OS="$(uname -s)"
ARIADNE_HOME="${ARIADNE_HOME:-$HOME/.ariadne}"
MANIFEST="$ARIADNE_HOME/install.env"
LOG_FILE="$ARIADNE_HOME/install.log"
PLIST_LABEL="dev.ariadne.daemon"
PLIST_PATH="$HOME/Library/LaunchAgents/$PLIST_LABEL.plist"
UNIT_PATH="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/ariadned.service"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
BASH_DIR="$DATA_DIR/bash-completion/completions"
ZSH_DIR="$DATA_DIR/zsh/site-functions"
ZSHRC="${ZDOTDIR:-$HOME}/.zshrc"

# Only a label for the step title and the summary; an OS with no service of
# ours still gets everything before that step, and fails inside it.
SERVICE_DESC=""
if [ "$WITH_SERVICE" = 1 ]; then
    case "$OS" in
        Darwin) SERVICE_DESC="launchd $PLIST_LABEL" ;;
        Linux) SERVICE_DESC="systemd --user ariadned.service" ;;
        *) SERVICE_DESC="unsupported on $OS" ;;
    esac
fi

# Remove a previously added "# >>> ariadne >>> ... # <<< ariadne <<<" block.
strip_block() {
    local file="$1"
    [ -f "$file" ] || return 0
    awk '/^# >>> ariadne >>>/{skip=1} skip==0{print} /^# <<< ariadne <<</{skip=0}' \
        "$file" > "$file.ariadne-tmp"
    mv "$file.ariadne-tmp" "$file"
}

# --- the plan ------------------------------------------------------------------
# One plan_add per step, in execution order; the step count adapts to the flags.
plan_add "Building release binaries"
plan_add "Stopping any running daemon"
plan_add "Installing binaries $UI_ARROW $(ui_tilde "$PREFIX")"
[ "$WITH_COMPLETIONS" = 1 ] && plan_add "Registering shell completions"
if [ "$WITH_SERVICE" = 1 ]; then
    plan_add "Registering the daemon service ($SERVICE_DESC)"
    plan_add "Waiting for the daemon"
fi
[ "$WITH_CODEX_HOOKS" = 1 ] && plan_add "Trusting Ariadne's Codex hooks"
plan_add "Writing the install manifest $UI_ARROW $(ui_tilde "$MANIFEST")"
ui_start

ui_header "Ariadne installer" \
    "repo    $REPO_DIR" \
    "prefix  $(ui_tilde "$PREFIX")" \
    "log     $(ui_tilde "$LOG_FILE")"

if [ "$UI_DRY_RUN" = 1 ]; then
    plan_print
    exit 0
fi

ui_log_init "$LOG_FILE"

# --- previous install (for cross-prefix idempotency) --------------------------
OLD_PREFIX=""
if [ -f "$MANIFEST" ]; then
    # shellcheck disable=SC1090
    OLD_PREFIX="$(. "$MANIFEST" && echo "${ARIADNE_PREFIX:-}")"
fi

# --- build ---------------------------------------------------------------------
step_begin
run_logged cargo build --release --manifest-path "$REPO_DIR/Cargo.toml" \
    || ui_die "cargo build failed"
step_ok

# --- stop whatever is currently running ------------------------------------------
step_begin
case "$OS" in
    Darwin) run_logged launchctl bootout "gui/$(id -u)/$PLIST_LABEL" || true ;;
    Linux) run_logged systemctl --user stop ariadned.service || true ;;
esac
if [ -f "$ARIADNE_HOME/ariadned.pid" ]; then
    kill "$(cat "$ARIADNE_HOME/ariadned.pid")" 2>/dev/null || true
fi
sleep 1
step_ok

# --- binaries --------------------------------------------------------------------
step_begin
mkdir -p "$PREFIX"
install -m 755 "$REPO_DIR/target/release/ariadne" "$PREFIX/ariadne"
install -m 755 "$REPO_DIR/target/release/ariadned" "$PREFIX/ariadned"
if [ -n "$OLD_PREFIX" ] && [ "$OLD_PREFIX" != "$PREFIX" ]; then
    rm -f "$OLD_PREFIX/ariadne" "$OLD_PREFIX/ariadned"
    step_ok "previous prefix $(ui_tilde "$OLD_PREFIX") cleaned"
else
    step_ok
fi

# --- completions ------------------------------------------------------------------
COMPLETION_SHELLS=""
if [ "$WITH_COMPLETIONS" = 1 ]; then
    step_begin
    # Completions are dynamic: the shell sources a shim that calls back into
    # the ariadne binary on TAB, which queries the daemon for live candidates
    # (task/goal/session ids, profile names). Remove static files from older
    # installs so they cannot shadow the dynamic registration.
    rm -f "$BASH_DIR/ariadne" "$ZSH_DIR/_ariadne"

    if [ -f "$HOME/.bashrc" ]; then
        strip_block "$HOME/.bashrc"
        cat >> "$HOME/.bashrc" <<EOF
# >>> ariadne >>>
[ -x "$PREFIX/ariadne" ] && source <(COMPLETE=bash "$PREFIX/ariadne")
# <<< ariadne <<<
EOF
        COMPLETION_SHELLS="bash"
    fi

    if [ -f "$ZSHRC" ]; then
        strip_block "$ZSHRC"
        # compdef only exists after compinit; the guard keeps shells without
        # compsys working.
        cat >> "$ZSHRC" <<EOF
# >>> ariadne >>>
if [ -x "$PREFIX/ariadne" ] && (( \$+functions[compdef] )); then
    source <(COMPLETE=zsh "$PREFIX/ariadne")
fi
# <<< ariadne <<<
EOF
        COMPLETION_SHELLS="${COMPLETION_SHELLS:+$COMPLETION_SHELLS, }zsh"
    fi

    if [ -n "$COMPLETION_SHELLS" ]; then
        step_ok "$COMPLETION_SHELLS - new shells only"
    else
        step_skip "no ~/.bashrc or ~/.zshrc found"
    fi
fi

# --- daemon service -----------------------------------------------------------------
DAEMON_STATE="not started"
if [ "$WITH_SERVICE" = 1 ]; then
    step_begin
    mkdir -p "$ARIADNE_HOME"
    case "$OS" in
        Darwin)
            mkdir -p "$(dirname "$PLIST_PATH")"
            cat > "$PLIST_PATH" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>$PLIST_LABEL</string>
    <key>ProgramArguments</key>
    <array><string>$PREFIX/ariadned</string></array>
    <key>EnvironmentVariables</key>
    <dict>
        <!-- launchd services get a bare PATH; the daemon needs tmux, git and
             the agent CLIs, so bake in the installing user's PATH. -->
        <key>PATH</key><string>$PATH</string>
    </dict>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key>
    <dict><key>SuccessfulExit</key><false/></dict>
    <key>ThrottleInterval</key><integer>10</integer>
    <key>StandardOutPath</key><string>$ARIADNE_HOME/ariadned.log</string>
    <key>StandardErrorPath</key><string>$ARIADNE_HOME/ariadned.log</string>
</dict>
</plist>
EOF
            run_logged launchctl bootstrap "gui/$(id -u)" "$PLIST_PATH" \
                || ui_die "launchctl bootstrap failed"
            ;;
        Linux)
            mkdir -p "$(dirname "$UNIT_PATH")"
            cat > "$UNIT_PATH" <<EOF
[Unit]
Description=Ariadne coding-agent orchestrator daemon

[Service]
ExecStart=$PREFIX/ariadned
# systemd user services also get a minimal PATH.
Environment="PATH=$PATH"
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
EOF
            run_logged systemctl --user daemon-reload \
                || ui_die "systemctl --user daemon-reload failed"
            run_logged systemctl --user enable ariadned.service || true
            run_logged systemctl --user restart ariadned.service \
                || ui_die "systemctl --user restart ariadned.service failed"
            ;;
        *)
            # No log tail to show: nothing ran, the OS is simply not one of ours.
            step_fail
            ui_error "unsupported OS for service setup: $OS (use --no-service and run ariadned yourself)"
            exit 1
            ;;
    esac
    step_ok

    step_begin
    for _ in $(seq 1 30); do
        if run_logged "$PREFIX/ariadne" daemon status; then
            DAEMON_STATE="running"
            break
        fi
        sleep 1
    done
    if [ "$DAEMON_STATE" = "running" ]; then
        step_ok
    else
        ui_die "no answer after 30s - see $(ui_tilde "$ARIADNE_HOME/ariadned.log")"
    fi
fi

# --- codex hooks -----------------------------------------------------------------------
# Codex carries its hooks per session, but only runs them once the user has
# trusted them — and it asks at the start of a session. The last step of the
# install therefore opens one, with the very flags the daemon will spawn with,
# so the user can answer. Nothing is written to ~/.codex by us.
CODEX_STATE="skipped"
if [ "$WITH_CODEX_HOOKS" = 1 ]; then
    step_begin
    if [ "$UI_YES" = 1 ]; then
        step_skip "--yes: run 'ariadne setup codex-hooks' when convenient"
    else
        run_interactive "$PREFIX/ariadne" setup codex-hooks --cli-bin "$PREFIX/ariadne" || true
        CODEX_STATE="prompted"
        step_ok
    fi
fi

# --- manifest (read by uninstall.sh) ---------------------------------------------------
step_begin
mkdir -p "$ARIADNE_HOME"
cat > "$MANIFEST" <<EOF
# Written by scripts/install.sh — read by scripts/uninstall.sh.
ARIADNE_PREFIX="$PREFIX"
ARIADNE_BASH_COMPLETION="$BASH_DIR/ariadne"
ARIADNE_ZSH_COMPLETION="$ZSH_DIR/_ariadne"
ARIADNE_PLIST="$PLIST_PATH"
ARIADNE_UNIT="$UNIT_PATH"
EOF
step_ok

# --- summary ----------------------------------------------------------------------------
printf '\n%sAriadne installed.%s\n\n' "$UI_B$UI_GREEN" "$UI_R"
ui_field "binaries" "$(ui_tilde "$PREFIX")/{ariadne,ariadned}"
if [ "$WITH_SERVICE" = 1 ]; then
    ui_field "service" "$SERVICE_DESC - daemon $DAEMON_STATE"
else
    ui_field "service" "not registered (--no-service) - run ariadned yourself"
fi
if [ "$WITH_COMPLETIONS" = 1 ]; then
    ui_field "completions" "${COMPLETION_SHELLS:-none - no ~/.bashrc or ~/.zshrc}"
else
    ui_field "completions" "not installed (--no-completions)"
fi
[ "$WITH_CODEX_HOOKS" = 1 ] && ui_field "codex hooks" "$CODEX_STATE"
ui_field "manifest" "$(ui_tilde "$MANIFEST")"
ui_field "log" "$(ui_tilde "$LOG_FILE")"
printf '\n'

case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *) ui_warn "$PREFIX is not on your PATH - add: export PATH=\"$PREFIX:\$PATH\"" ;;
esac

if [ "$WITH_SERVICE" = 1 ]; then
    case "$OS" in
        Darwin) STOP_HINT="launchctl bootout gui/\$(id -u)/$PLIST_LABEL" ;;
        Linux) STOP_HINT="systemctl --user stop ariadned" ;;
    esac
    printf '  %sthe daemon restarts on failure; stop it with: %s%s\n' \
        "$UI_D" "$STOP_HINT" "$UI_R"
fi
printf "  %sTry:%s %sariadne goal create --title '...' --repo /path/to/repo%s\n\n" \
    "$UI_D" "$UI_R" "$UI_CYAN" "$UI_R"
