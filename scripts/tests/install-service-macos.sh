#!/usr/bin/env bash
# Run on macOS: bash scripts/tests/install-service-macos.sh
#
# Proves an upgrade survives a launchd service slow to drop: the fake
# launchctl (scripts/tests/install-command.sh) keeps the label in `print`
# for a couple of polls after `bootout` returns, and fails the first
# `bootstrap` with the real "Bootstrap failed: 5: Input/output error". The
# install must wait the first out and retry the second, ending with the
# service registered rather than calling ui_die. scripts/AGENTS.md.
set -Eeuo pipefail
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
[ "$(uname -s)" = Darwin ] || { echo "macOS only" >&2; exit 2; }
export TEST_ROOT
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT
trap 'printf "FAIL (line %s)\n" "$LINENO"; cat "$TEST_ROOT/output" 2>/dev/null' ERR
mkdir -p "$TEST_ROOT"/{repo/scripts,bin,payload,home}
cp "$REPO_DIR/scripts/"{install.sh,uninstall.sh,lib.sh} "$TEST_ROOT/repo/scripts/"
for tool in git cargo security codesign launchctl; do
    cp "$REPO_DIR/scripts/tests/install-command.sh" "$TEST_ROOT/bin/$tool"
    chmod 755 "$TEST_ROOT/bin/$tool"
done
for tool in ariadne ariadned; do
    cp "$REPO_DIR/scripts/tests/install-command.sh" "$TEST_ROOT/payload/$tool"
    chmod 755 "$TEST_ROOT/payload/$tool"
done
export CARGO_TARGET_DIR="$TEST_ROOT/build"
export PATH="$TEST_ROOT/bin:$PATH"
export TEST_IDENTITY=missing

env HOME="$TEST_ROOT/home" ARIADNE_HOME="$TEST_ROOT/home/.ariadne" \
    PREFIX="$TEST_ROOT/home/.local/bin" \
    bash "$TEST_ROOT/repo/scripts/install.sh" --build-from-source \
        --no-completions --no-ui --yes > "$TEST_ROOT/output" 2>&1

# Both waits ran: launchd kept reporting the label present for a couple of
# polls, and the first bootstrap lost the race before the second won it.
[ "$(grep -c '^launchctl print' "$TEST_ROOT/commands")" -ge 2 ]
[ "$(grep -c '^launchctl bootstrap' "$TEST_ROOT/commands")" -ge 2 ]
grep -Fq 'Bootstrap failed: 5: Input/output error' "$TEST_ROOT/home/.ariadne/install.log"
grep -E 'OK.*Registering the daemon service' "$TEST_ROOT/output"
test -x "$TEST_ROOT/home/.local/bin/ariadne"
test -x "$TEST_ROOT/home/.local/bin/ariadned"

printf 'PASS: slow-bootout-and-failed-bootstrap\n'
