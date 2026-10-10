#!/usr/bin/env bash
# Run on macOS: bash scripts/tests/install-sign-macos.sh CASE, for one of:
#   present          an "Ariadne Code Signing" identity is in the login
#                     keychain: install.sh --build-from-source signs
#                     ariadne and ariadned with it.
#   missing           no identity is there: it skips with a step note,
#                     never failing the install.
#   desktop-present    the identity is there: install.sh passes it to
#                      npm run tauri build as APPLE_SIGNING_IDENTITY, for
#                      Tauri to sign the .app with.
#   desktop-missing    no identity: APPLE_SIGNING_IDENTITY reaches npm
#                       unset, as it always did.
# desktop-* stop npm's stub right after it records what reached its own
# environment (scripts/tests/install-command.sh): the real next step, ditto
# to /Applications, is a system path no sandboxed test may touch, so these
# two prove the wiring and go no further. scripts/AGENTS.md.
set -Eeuo pipefail
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
test_case="${1:?name a test case: present, missing, desktop-present or desktop-missing}"
[ "$(uname -s)" = Darwin ] || { echo "macOS only" >&2; exit 2; }
export TEST_ROOT
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT
trap 'printf "FAIL: %s (line %s)\n" "$test_case" "$LINENO"; cat "$TEST_ROOT/output" 2>/dev/null' ERR
mkdir -p "$TEST_ROOT"/{repo/scripts,repo/ui,bin,payload,home}
cp "$REPO_DIR/scripts/"{install.sh,uninstall.sh,lib.sh} "$TEST_ROOT/repo/scripts/"
cp "$REPO_DIR/ui/package.json" "$TEST_ROOT/repo/ui/"
for tool in git cargo security codesign npm; do
    cp "$REPO_DIR/scripts/tests/install-command.sh" "$TEST_ROOT/bin/$tool"
    chmod 755 "$TEST_ROOT/bin/$tool"
done
for tool in ariadne ariadned; do
    cp "$REPO_DIR/scripts/tests/install-command.sh" "$TEST_ROOT/payload/$tool"
    chmod 755 "$TEST_ROOT/payload/$tool"
done
export CARGO_TARGET_DIR="$TEST_ROOT/build"
export PATH="$TEST_ROOT/bin:$PATH"

case "$test_case" in
    present|missing)
        export TEST_IDENTITY="$test_case"
        env HOME="$TEST_ROOT/home" ARIADNE_HOME="$TEST_ROOT/home/.ariadne" \
            PREFIX="$TEST_ROOT/home/.local/bin" \
            bash "$TEST_ROOT/repo/scripts/install.sh" --build-from-source --no-service \
                --no-completions --no-ui > "$TEST_ROOT/output" 2>&1
        case "$test_case" in
            present)
                grep -Fx "codesign --force --options runtime -s Ariadne Code Signing $TEST_ROOT/build/release/ariadne $TEST_ROOT/build/release/ariadned" \
                    "$TEST_ROOT/commands"
                grep -E 'OK.*Building release binaries.*signed with Ariadne Code Signing' "$TEST_ROOT/output"
                ;;
            missing)
                if grep '^codesign ' "$TEST_ROOT/commands"; then exit 1; fi
                grep -E 'OK.*Building release binaries.*not signed.*make-signing-cert\.sh' "$TEST_ROOT/output"
                ;;
        esac
        test -x "$TEST_ROOT/home/.local/bin/ariadne"
        test -x "$TEST_ROOT/home/.local/bin/ariadned"
        ;;
    desktop-present|desktop-missing)
        export TEST_IDENTITY="${test_case#desktop-}"
        # Expected to fail: npm's stub exits 1 right after recording what it
        # saw, on purpose, rather than go on to ditto a bundle to
        # /Applications.
        if env HOME="$TEST_ROOT/home" ARIADNE_HOME="$TEST_ROOT/home/.ariadne" \
            PREFIX="$TEST_ROOT/home/.local/bin" \
            bash "$TEST_ROOT/repo/scripts/install.sh" --build-from-source --no-service \
                --no-completions > "$TEST_ROOT/output" 2>&1; then
            echo "expected the stubbed npm run tauri build to fail" >&2
            exit 1
        fi
        case "$test_case" in
            desktop-present) grep -Fxq 'Ariadne Code Signing' "$TEST_ROOT/apple-signing-identity" ;;
            desktop-missing) [ ! -s "$TEST_ROOT/apple-signing-identity" ] ;;
        esac
        ;;
    *) exit 2 ;;
esac
printf 'PASS: %s\n' "$test_case"
