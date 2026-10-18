#!/usr/bin/env bash
# External tools for scripts/make-signing-cert.sh's boundary tests. No real
# keychain, network or cryptography is used: the "keychain" is two marker
# files under $TEST_ROOT (cert-present, identity-valid), and a PKCS#12 file
# is a one-line stand-in holding its own password, so a test can check the
# password it was given is the one that reached the file.
set -euo pipefail
tool="$(basename "$0")"
printf '%s %s\n' "$tool" "$*" >> "$TEST_ROOT/commands"
CERT_MARKER="$TEST_ROOT/cert-present"
VALID_MARKER="$TEST_ROOT/identity-valid"

case "$tool" in
    openssl)
        sub="$1"; shift
        case "$sub" in
            req)
                keyout="" out=""
                while [ $# -gt 0 ]; do
                    case "$1" in
                        -keyout) keyout="$2"; shift 2 ;;
                        -out) out="$2"; shift 2 ;;
                        *) shift ;;
                    esac
                done
                printf 'KEY\n' > "$keyout"
                printf 'CERT\n' > "$out"
                ;;
            pkcs12)
                out="" passout=""
                while [ $# -gt 0 ]; do
                    case "$1" in
                        -out) out="$2"; shift 2 ;;
                        -passout) passout="$2"; shift 2 ;;
                        *) shift ;;
                    esac
                done
                cat "${passout#file:}" > "$out"
                ;;
            rand) printf 'test-generated-password\n' ;;
            *) exit 1 ;;
        esac
        ;;
    security)
        sub="$1"; shift
        case "$sub" in
            find-certificate)
                [ -f "$CERT_MARKER" ] || exit 44
                ;;
            find-identity)
                if [ -f "$VALID_MARKER" ]; then
                    printf '  1) 0000000000000000000000000000000000000000 "Ariadne Code Signing"\n'
                    printf '     1 valid identities found\n'
                else
                    printf '     0 valid identities found\n'
                fi
                ;;
            delete-identity|delete-certificate)
                [ -f "$CERT_MARKER" ] || exit 1
                rm -f "$CERT_MARKER" "$VALID_MARKER"
                ;;
            import)
                file="$1"; shift
                password=""
                while [ $# -gt 0 ]; do
                    case "$1" in
                        -P) password="$2"; shift 2 ;;
                        *) shift ;;
                    esac
                done
                expected="$(cat "$file" 2>/dev/null || true)"
                [ "$password" = "$expected" ] || { echo "MAC verification failed" >&2; exit 1; }
                : > "$CERT_MARKER"
                : > "$VALID_MARKER"
                ;;
            add-trusted-cert) ;;
            *) exit 1 ;;
        esac
        ;;
    gh)
        case "${1:-} ${2:-}" in
            'secret set')
                if [ "${3:-}" = APPLE_CERTIFICATE ]; then
                    cat > /dev/null
                fi
                ;;
            *) exit 1 ;;
        esac
        ;;
    *) exit 1 ;;
esac
