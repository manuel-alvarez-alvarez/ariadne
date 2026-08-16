#!/usr/bin/env bash
# Ariadne installer: builds from source, installs the binaries, registers the
# daemon as a user service (launchd on macOS, systemd --user on Linux),
# installs bash/zsh completions and has the user trust Ariadne's Codex hooks.
#
# Idempotent: safe to re-run after upgrades or config changes; every step
# replaces what a previous run installed. What was installed where is
# recorded in ~/.ariadne/install.env, which uninstall.sh reads.
#
# Usage: scripts/install.sh [--prefix DIR] [--no-service] [--no-completions]
#                           [--no-codex-hooks]
#   --prefix DIR       install binaries into DIR (default: ~/.local/bin)
#   --no-service       skip daemon service registration
#   --no-completions   skip shell completion installation
#   --no-codex-hooks   skip the Codex hook trust prompt
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local/bin}"
WITH_SERVICE=1
WITH_COMPLETIONS=1
WITH_CODEX_HOOKS=1
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --no-service) WITH_SERVICE=0; shift ;;
        --no-completions) WITH_COMPLETIONS=0; shift ;;
        --no-codex-hooks) WITH_CODEX_HOOKS=0; shift ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OS="$(uname -s)"
ARIADNE_HOME="${ARIADNE_HOME:-$HOME/.ariadne}"
MANIFEST="$ARIADNE_HOME/install.env"
PLIST_LABEL="dev.ariadne.daemon"
PLIST_PATH="$HOME/Library/LaunchAgents/$PLIST_LABEL.plist"
UNIT_PATH="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/ariadned.service"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }

# Remove a previously added "# >>> ariadne >>> ... # <<< ariadne <<<" block.
strip_block() {
    local file="$1"
    [ -f "$file" ] || return 0
    awk '/^# >>> ariadne >>>/{skip=1} skip==0{print} /^# <<< ariadne <<</{skip=0}' \
        "$file" > "$file.ariadne-tmp"
    mv "$file.ariadne-tmp" "$file"
}

# --- previous install (for cross-prefix idempotency) --------------------------
OLD_PREFIX=""
if [ -f "$MANIFEST" ]; then
    # shellcheck disable=SC1090
    OLD_PREFIX="$(. "$MANIFEST" && echo "${ARIADNE_PREFIX:-}")"
fi

# --- build ---------------------------------------------------------------------
say "building release binaries"
cargo build --release --manifest-path "$REPO_DIR/Cargo.toml"

# --- stop whatever is currently running ------------------------------------------
say "stopping any running daemon"
case "$OS" in
    Darwin) launchctl bootout "gui/$(id -u)/$PLIST_LABEL" 2>/dev/null || true ;;
    Linux) systemctl --user stop ariadned.service 2>/dev/null || true ;;
esac
if [ -f "$ARIADNE_HOME/ariadned.pid" ]; then
    kill "$(cat "$ARIADNE_HOME/ariadned.pid")" 2>/dev/null || true
fi
sleep 1

# --- binaries --------------------------------------------------------------------
say "installing binaries to $PREFIX"
mkdir -p "$PREFIX"
install -m 755 "$REPO_DIR/target/release/ariadne" "$PREFIX/ariadne"
install -m 755 "$REPO_DIR/target/release/ariadned" "$PREFIX/ariadned"
if [ -n "$OLD_PREFIX" ] && [ "$OLD_PREFIX" != "$PREFIX" ]; then
    say "removing binaries from previous prefix $OLD_PREFIX"
    rm -f "$OLD_PREFIX/ariadne" "$OLD_PREFIX/ariadned"
fi

# --- completions ------------------------------------------------------------------
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
BASH_DIR="$DATA_DIR/bash-completion/completions"
ZSH_DIR="$DATA_DIR/zsh/site-functions"
ZSHRC="${ZDOTDIR:-$HOME}/.zshrc"
if [ "$WITH_COMPLETIONS" = 1 ]; then
    say "registering dynamic shell completions"
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
        say "completion registered in $ZSHRC (takes effect in new shells)"
    fi
fi

# --- daemon service -----------------------------------------------------------------
if [ "$WITH_SERVICE" = 1 ]; then
    mkdir -p "$ARIADNE_HOME"
    case "$OS" in
        Darwin)
            say "registering launchd service $PLIST_LABEL"
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
            launchctl bootstrap "gui/$(id -u)" "$PLIST_PATH"
            ;;
        Linux)
            say "registering systemd user service ariadned"
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
            systemctl --user daemon-reload
            systemctl --user enable ariadned.service >/dev/null 2>&1 || true
            systemctl --user restart ariadned.service
            ;;
        *)
            echo "unsupported OS for service setup: $OS (use --no-service and run ariadned yourself)" >&2
            exit 1
            ;;
    esac

    say "waiting for the daemon"
    for _ in $(seq 1 30); do
        if "$PREFIX/ariadne" daemon status >/dev/null 2>&1; then break; fi
        sleep 1
    done
    "$PREFIX/ariadne" daemon status
fi

# --- codex hooks -----------------------------------------------------------------------
# Codex carries its hooks per session, but only runs them once the user has
# trusted them — and it asks at the start of a session. The last step of the
# install therefore opens one, with the very flags the daemon will spawn with,
# so the user can answer. Nothing is written to ~/.codex by us.
if [ "$WITH_CODEX_HOOKS" = 1 ]; then
    say "trusting Ariadne's Codex hooks"
    "$PREFIX/ariadne" setup codex-hooks --cli-bin "$PREFIX/ariadne" || true
fi

# --- manifest (read by uninstall.sh) ---------------------------------------------------
mkdir -p "$ARIADNE_HOME"
cat > "$MANIFEST" <<EOF
# Written by scripts/install.sh — read by scripts/uninstall.sh.
ARIADNE_PREFIX="$PREFIX"
ARIADNE_BASH_COMPLETION="$BASH_DIR/ariadne"
ARIADNE_ZSH_COMPLETION="$ZSH_DIR/_ariadne"
ARIADNE_PLIST="$PLIST_PATH"
ARIADNE_UNIT="$UNIT_PATH"
EOF

case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *) say "NOTE: $PREFIX is not on your PATH — add: export PATH=\"$PREFIX:\$PATH\"" ;;
esac

say "installed. Try: ariadne goal create --title '...' --repo /path/to/repo"
if [ "$WITH_SERVICE" = 1 ]; then
    case "$OS" in
        Darwin) say "daemon runs under launchd (auto-restart on failure); stop: launchctl bootout gui/\$(id -u)/$PLIST_LABEL" ;;
        Linux) say "daemon runs under systemd --user (auto-restart on failure); stop: systemctl --user stop ariadned" ;;
    esac
fi
