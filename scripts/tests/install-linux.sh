#!/usr/bin/env bash
# Run on Linux: bash scripts/tests/install-linux.sh CASE [host|present|missing].
# The host option uses the real linker cache, including in Ubuntu containers.
# TEST_PAYLOAD_DIR can supply native ariadne, ariadned and ariadne-desktop binaries.
set -Eeuo pipefail
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
test_case="${1:?name a test case}"
export TEST_WEBKIT="${2:-present}"
export TEST_ROOT
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT
trap 'printf "FAIL: %s (line %s)\n" "$test_case" "$LINENO"; cat "$TEST_ROOT/output" 2>/dev/null' ERR
mkdir -p "$TEST_ROOT"/{repo/scripts,repo/ui/src-tauri/icons,bin,assets,payload,home}
cp "$REPO_DIR/scripts/"{install.sh,uninstall.sh,lib.sh} "$TEST_ROOT/repo/scripts/"
cp "$REPO_DIR/ui/package.json" "$TEST_ROOT/repo/ui/"
cp "$REPO_DIR/ui/src-tauri/icons/icon.png" "$TEST_ROOT/repo/ui/src-tauri/icons/"
# Include both service managers. The harness is normally Linux-only, but a
# mistaken host run must still use stand-ins and never reach a user service.
for tool in gh git ldconfig cargo npm tar systemctl launchctl sleep gtk-update-icon-cache update-desktop-database; do
    cp "$REPO_DIR/scripts/tests/install-command.sh" "$TEST_ROOT/bin/$tool"
    chmod 755 "$TEST_ROOT/bin/$tool"
done
for tool in ariadne ariadned ariadne-desktop; do
    if [ -n "${TEST_PAYLOAD_DIR:-}" ]; then
        cp "$TEST_PAYLOAD_DIR/$tool" "$TEST_ROOT/payload/$tool"
    else
        cp "$REPO_DIR/scripts/tests/install-command.sh" "$TEST_ROOT/payload/$tool"
    fi
    chmod 755 "$TEST_ROOT/payload/$tool"
done
cp "$REPO_DIR/ui/src-tauri/icons/icon.png" "$TEST_ROOT/payload/icon.png"
target="$(uname -m)-unknown-linux-gnu"
tar -czf "$TEST_ROOT/assets/ariadne-$target.tar.gz" -C "$TEST_ROOT/payload" ariadne ariadned
tar -czf "$TEST_ROOT/assets/ariadne-desktop-$target.tar.gz" -C "$TEST_ROOT/payload" ariadne-desktop icon.png
touch "$TEST_ROOT/home/.bashrc" "$TEST_ROOT/home/.zshrc"
export CARGO_TARGET_DIR="$TEST_ROOT/build"
export PATH="$TEST_ROOT/bin:$PATH"
# Keep the caller's HOME unchanged. Only the installer subprocess gets a test home.
run_install() {
    env HOME="$TEST_ROOT/home" ARIADNE_HOME="$TEST_ROOT/home/.ariadne" \
        XDG_DATA_HOME="$TEST_ROOT/home/.local/share" ZDOTDIR="$TEST_ROOT/home" \
        PREFIX="$TEST_ROOT/home/.local/bin" \
        bash "$TEST_ROOT/repo/scripts/install.sh" --no-service "$@" > "$TEST_ROOT/output" 2>&1
}
run_uninstall() {
    env HOME="$TEST_ROOT/home" ARIADNE_HOME="$TEST_ROOT/home/.ariadne" \
        XDG_DATA_HOME="$TEST_ROOT/home/.local/share" ZDOTDIR="$TEST_ROOT/home" \
        bash "$TEST_ROOT/repo/scripts/uninstall.sh" > "$TEST_ROOT/uninstall-output" 2>&1
}
prefix="$TEST_ROOT/home/.local/bin"
entry="$TEST_ROOT/home/.local/share/applications/dev.ariadne.ui.desktop"
case "$test_case" in
    missing|source-missing)
        if [ "$test_case" = source-missing ]; then run_install --build-from-source; else run_install; fi
        grep -E 'SKIP.*Installing|SKIP.*Building and installing' "$TEST_ROOT/output"
        for hint in 'apt install libwebkit2gtk-4.1-0' 'dnf install webkit2gtk4.1' 'pacman -S webkit2gtk-4.1'; do
            grep -F "$hint" "$TEST_ROOT/output"
        done
        grep -E 'desktop app +skipped.*libwebkit2gtk-4.1.so.0' "$TEST_ROOT/output"
        grep -E 'SKIP.*GNOME.*was not installed' "$TEST_ROOT/output"
        [ ! -e "$prefix/ariadne-desktop" ]
        [ ! -e "$entry" ]
        if grep -E 'gh release download.*ariadne-desktop|npm |ARIADNE_APP=' \
            "$TEST_ROOT/commands" "$TEST_ROOT/home/.ariadne/install.env"; then exit 1; fi
        ;;
    release|source)
        if [ "$test_case" = source ]; then
            run_install --build-from-source
            grep -Fx 'npm run tauri build -- --no-bundle' "$TEST_ROOT/commands"
        else
            # Force the release to supply its own icon.
            rm "$TEST_ROOT/repo/ui/src-tauri/icons/icon.png"
            run_install
        fi
        cmp "$TEST_ROOT/payload/ariadne-desktop" "$prefix/ariadne-desktop"
        [ "$(stat -c %a "$prefix/ariadne-desktop")" = 755 ]
        grep -Fx 'StartupWMClass=ariadne-desktop' "$entry"
        grep -Fx "Exec=\"$prefix/ariadne-desktop\" %U" "$entry"
        icon="$(find "$TEST_ROOT/home/.local/share/icons" -name dev.ariadne.ui.png)"
        cmp "$TEST_ROOT/payload/icon.png" "$icon"
        ;;
    rejected)
        export TEST_REJECT=1
        if run_install; then exit 1; fi
        grep -F 'failed attestation verification' "$TEST_ROOT/output"
        if grep '^tar ' "$TEST_ROOT/commands"; then exit 1; fi
        [ ! -e "$prefix/ariadne" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    workflow-rejected)
        export TEST_ATTESTATION_WORKFLOW='test/install/.github/workflows/other.yml'
        if run_install; then exit 1; fi
        grep -F 'failed attestation verification' "$TEST_ROOT/output"
        [ ! -e "$prefix/ariadne" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    tag-rejected)
        export TEST_ATTESTATION_REF='refs/tags/v9.9.9'
        if run_install; then exit 1; fi
        grep -F 'failed attestation verification' "$TEST_ROOT/output"
        [ ! -e "$prefix/ariadne" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    gh-missing-attestation-flags)
        export TEST_GH_ATTESTATION_FLAGS=missing
        if run_install; then exit 1; fi
        grep -F 'gh 2.102.0 or later is required for attestation verification' "$TEST_ROOT/output"
        [ ! -e "$prefix/ariadne" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    gh-outdated-attestation-verifier)
        export TEST_GH_VERSION=2.101.0
        if run_install; then exit 1; fi
        grep -F 'gh 2.102.0 or later is required for attestation verification' "$TEST_ROOT/output"
        [ ! -e "$prefix/ariadne" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    manifest-shell)
        run_install
        marker="$TEST_ROOT/manifest-shell-ran"
        # shellcheck disable=SC2016
        printf 'ARIADNE_PREFIX=$(touch %s)\n' "$marker" > "$TEST_ROOT/home/.ariadne/install.env"
        if run_uninstall; then exit 1; fi
        grep -F 'unsafe value in install manifest' "$TEST_ROOT/uninstall-output"
        [ ! -e "$marker" ]
        [ -e "$prefix/ariadne" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    manifest-unexpected)
        run_install
        printf 'ARIADNE_PREFIX=%s\nUNEXPECTED=value\n' "$prefix" > "$TEST_ROOT/home/.ariadne/install.env"
        if run_uninstall; then exit 1; fi
        grep -F 'unexpected key in install manifest' "$TEST_ROOT/uninstall-output"
        [ -e "$prefix/ariadne" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    app-path)
        run_install
        outside="$TEST_ROOT/outside/ariadne-desktop"
        mkdir -p "$(dirname "$outside")"
        : > "$outside"
        sed -i "s|^ARIADNE_APP=.*|ARIADNE_APP=$outside|" "$TEST_ROOT/home/.ariadne/install.env"
        if run_uninstall; then exit 1; fi
        grep -F 'refusing to remove unexpected desktop app path' "$TEST_ROOT/uninstall-output"
        [ -e "$outside" ]
        [ -e "$prefix/ariadne" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    purge-home)
        purge_home="$TEST_ROOT/purge-home"
        mkdir -p "$purge_home"
        : > "$purge_home/keep"
        if env HOME="$purge_home" ARIADNE_HOME="$purge_home" \
            bash "$TEST_ROOT/repo/scripts/uninstall.sh" --purge > "$TEST_ROOT/uninstall-output" 2>&1; then
            exit 1
        fi
        grep -F 'refusing to purge unsafe Ariadne home' "$TEST_ROOT/uninstall-output"
        [ -e "$purge_home/keep" ]
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    purge-root)
        if env HOME="$TEST_ROOT/home" ARIADNE_HOME=/ \
            bash "$TEST_ROOT/repo/scripts/uninstall.sh" --purge --dry-run > "$TEST_ROOT/uninstall-output" 2>&1; then
            exit 1
        fi
        grep -F 'refusing to purge unsafe Ariadne home' "$TEST_ROOT/uninstall-output"
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    pid-invalid)
        # `kill` is a shell function only in this test process. It records an
        # attempted signal, so the old unsafe implementation cannot reach a
        # real process while this scenario proves the public script helper.
        # shellcheck source=/dev/null
        . "$TEST_ROOT/repo/scripts/lib.sh"
        ui_init
        # shellcheck disable=SC2034
        OS=Other
        ARIADNE_HOME="$TEST_ROOT/home/.ariadne"
        mkdir -p "$ARIADNE_HOME"
        # shellcheck disable=SC2329
        kill() {
            printf '%s\n' "$*" >> "$TEST_ROOT/kills"
            [ "${1:-}" = -0 ] && return 1
            return 0
        }
        for invalid_pid in -1 word '' ' 2' $'2\n\n'; do
            printf '%s\n' "$invalid_pid" > "$ARIADNE_HOME/ariadned.pid"
            rm -f "$TEST_ROOT/kills"
            ui_stop_daemon
            [ ! -e "$TEST_ROOT/kills" ]
        done
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    uninstall-log)
        uninstall_tmp="$TEST_ROOT/uninstall-tmp"
        mkdir -p "$uninstall_tmp"
        env HOME="$TEST_ROOT/home" ARIADNE_HOME="$TEST_ROOT/home/.ariadne" TMPDIR="$uninstall_tmp" \
            bash "$TEST_ROOT/repo/scripts/uninstall.sh" > "$TEST_ROOT/uninstall-output" 2>&1
        uninstall_log="$(find "$uninstall_tmp" -type f -name 'ariadne-uninstall.*' -print)"
        case "$(basename "$uninstall_log")" in
            ariadne-uninstall.??????) ;;
            *) exit 1 ;;
        esac
        printf 'PASS: %s\n' "$test_case"
        exit 0
        ;;
    no-ui)
        run_install --no-ui
        if grep '^ldconfig ' "$TEST_ROOT/commands"; then exit 1; fi
        if grep 'gh release download.*ariadne-desktop' "$TEST_ROOT/commands"; then exit 1; fi
        ;;
    *) exit 2 ;;
esac
test -x "$prefix/ariadne"
test -x "$prefix/ariadned"
grep -F '# >>> ariadne >>>' "$TEST_ROOT/home/.bashrc" "$TEST_ROOT/home/.zshrc"
run_uninstall
[ ! -e "$prefix/ariadne-desktop" ]
[ ! -e "$prefix/ariadne" ]
[ ! -e "$prefix/ariadned" ]
[ ! -e "$entry" ]
if [ -n "${icon:-}" ]; then [ ! -e "$icon" ]; fi
printf 'PASS: %s\n' "$test_case"
