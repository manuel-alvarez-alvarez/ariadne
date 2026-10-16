#!/usr/bin/env bash
# External tools for the installer boundary tests. No network or compiler is used.
set -euo pipefail
tool="$(basename "$0")"
printf '%s %s\n' "$tool" "$*" >> "$TEST_ROOT/commands"
case "$tool" in
    gh)
        if [ "${1:-}" = --version ]; then
            printf 'gh version %s\n' "${TEST_GH_VERSION:-2.102.0}"
            exit 0
        fi
        case "${1:-} ${2:-}" in
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
                if [ "${3:-}" = --help ]; then
                    if [ "${TEST_GH_ATTESTATION_FLAGS:-present}" = missing ]; then
                        printf '%s\n' 'usage: gh attestation verify [flags]'
                    else
                        printf '%s\n' '--signer-workflow string'
                        printf '%s\n' '--source-ref string'
                    fi
                    exit 0
                fi
                asset="$3"
                if [ "${TEST_REJECT:-0}" = 1 ] && [[ "$asset" = *ariadne-desktop-* ]]; then
                    exit 1
                fi
                expected_workflow='test/install/.github/workflows/release.yml'
                expected_ref='refs/tags/v0.0.0'
                workflow=''
                source_ref=''
                shift 3
                while [ $# -gt 0 ]; do
                    case "$1" in
                        --signer-workflow) workflow="$2"; shift 2 ;;
                        --source-ref) source_ref="$2"; shift 2 ;;
                        *) shift ;;
                    esac
                done
                # Without either pin, gh accepts the fixture's other signer
                # and tag, as the old installer did. With both, it applies
                # exactly the constraints the real verifier enforces.
                if [ -n "$workflow$source_ref" ]; then
                    [ "$workflow" = "$expected_workflow" ] || exit 1
                    [ "$source_ref" = "$expected_ref" ] || exit 1
                    [ "${TEST_ATTESTATION_WORKFLOW:-$expected_workflow}" = "$workflow" ] || exit 1
                    [ "${TEST_ATTESTATION_REF:-$expected_ref}" = "$source_ref" ] || exit 1
                fi
                printf '%s\n' "$asset" >> "$TEST_ROOT/verified"
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
    security)
        case "$*" in
            'find-identity -v -p codesigning')
                if [ "${TEST_IDENTITY:-missing}" = present ]; then
                    printf '  1) 0000000000000000000000000000000000000000 "Ariadne Code Signing"\n'
                    printf '     1 valid identities found\n'
                else
                    printf '     0 valid identities found\n'
                fi
                ;;
            *) exit 1 ;;
        esac
        ;;
    codesign) ;;
    npm)
        if [ "$1" = run ]; then
            case "$*" in
                'run tauri build -- --no-bundle')
                    cp "$TEST_ROOT/payload/ariadne-desktop" "$CARGO_TARGET_DIR/release/ariadne-ui"
                    ;;
                'run tauri build -- --bundles app')
                    # Real Tauri signs the real .app bundle and install.sh
                    # then ditto's it to /Applications - a system path no
                    # sandboxed test may touch. Recording what reached this
                    # stub's own environment, then failing, proves the
                    # wiring without going anywhere near that step.
                    printf '%s' "${APPLE_SIGNING_IDENTITY:-}" > "$TEST_ROOT/apple-signing-identity"
                    exit 1
                    ;;
                *) exit 1 ;;
            esac
        fi
        ;;
    ariadne)
        if [ "${1:-} ${2:-}" = 'completions install' ]; then
            printf '# >>> ariadne >>>\n# completion fixture\n# <<< ariadne <<<\n' >> "$HOME/.${4}rc"
        fi
        ;;
    launchctl)
        case "$1" in
            bootout) ;;
            print)
                # Keeps the label in print for TEST_PRINT_POLLS calls, then
                # reports it gone - a bootout that returned before launchd
                # actually dropped the service.
                n=0
                [ -f "$TEST_ROOT/launchctl-print-calls" ] \
                    && n="$(cat "$TEST_ROOT/launchctl-print-calls")"
                n=$((n + 1))
                printf '%s' "$n" > "$TEST_ROOT/launchctl-print-calls"
                [ "$n" -le "${TEST_PRINT_POLLS:-2}" ]
                ;;
            bootstrap)
                # Fails the first call the real way - "Bootstrap failed: 5:
                # Input/output error" - then succeeds, as it does once the
                # old job has actually cleared.
                n=0
                [ -f "$TEST_ROOT/launchctl-bootstrap-calls" ] \
                    && n="$(cat "$TEST_ROOT/launchctl-bootstrap-calls")"
                n=$((n + 1))
                printf '%s' "$n" > "$TEST_ROOT/launchctl-bootstrap-calls"
                if [ "$n" -eq 1 ]; then
                    echo "Bootstrap failed: 5: Input/output error" >&2
                    exit 5
                fi
                ;;
            *) exit 1 ;;
        esac
        ;;
    ariadned|ariadne-desktop|systemctl|sleep|gtk-update-icon-cache|update-desktop-database) ;;
    *) exit 1 ;;
esac
