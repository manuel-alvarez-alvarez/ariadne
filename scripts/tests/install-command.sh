#!/usr/bin/env bash
# External tools for the installer boundary tests. No network or compiler is used.
set -euo pipefail
tool="$(basename "$0")"
printf '%s %s\n' "$tool" "$*" >> "$TEST_ROOT/commands"
case "$tool" in
    gh)
        case "$1 $2" in
            'auth status') ;;
            'release view') printf 'v0.0.0\n' ;;
            'release download')
                shift 2
                while [ $# -gt 0 ]; do
                    case "$1" in
                        --dir) dest="$2"; shift 2 ;;
                        --pattern) asset="$2"; shift 2 ;;
                        *) shift ;;
                    esac
                done
                cp "$TEST_ROOT/assets/$asset" "$dest/$asset"
                ;;
            'attestation verify')
                if [ "${TEST_REJECT:-0}" = 1 ] && [[ "$3" = *ariadne-desktop-* ]]; then
                    exit 1
                fi
                printf '%s\n' "$3" >> "$TEST_ROOT/verified"
                ;;
            *) exit 1 ;;
        esac
        ;;
    tar)
        # An extraction before verification fails even when its output is unused.
        grep -Fx "$2" "$TEST_ROOT/verified"
        exec /usr/bin/tar "$@"
        ;;
    git) printf 'https://github.com/test/install.git\n' ;;
    ldconfig)
        case "$TEST_WEBKIT" in
            host) exec /sbin/ldconfig "$@" ;;
            present) printf 'libwebkit2gtk-4.1.so.0 (libc6) => /lib/libwebkit2gtk-4.1.so.0\n' ;;
            missing) ;;
            forbidden) exit 99 ;;
        esac
        ;;
    cargo)
        mkdir -p "$CARGO_TARGET_DIR/release"
        cp "$TEST_ROOT/payload/ariadne" "$TEST_ROOT/payload/ariadned" "$CARGO_TARGET_DIR/release/"
        ;;
    npm)
        if [ "$1" = run ]; then
            [ "$*" = 'run tauri build -- --no-bundle' ]
            cp "$TEST_ROOT/payload/ariadne-desktop" "$CARGO_TARGET_DIR/release/ariadne-ui"
        fi
        ;;
    ariadne)
        if [ "${1:-} ${2:-}" = 'completions install' ]; then
            printf '# >>> ariadne >>>\n# completion fixture\n# <<< ariadne <<<\n' >> "$HOME/.${4}rc"
        fi
        ;;
    ariadned|ariadne-desktop|systemctl|sleep|gtk-update-icon-cache|update-desktop-database) ;;
    *) exit 1 ;;
esac
