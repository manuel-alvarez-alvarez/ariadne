#!/usr/bin/env bash
# Run on macOS: bash scripts/tests/make-signing-cert-scenarios.sh CASE, for
# one of its cases: create, idempotent, replace, repair-invalid, export,
# secrets. Proves scripts/make-signing-cert.sh's own behavior against a
# stubbed openssl, security and gh on PATH
# (scripts/tests/make-signing-cert-command.sh) - no real keychain, network
# or cryptography involved.
set -Eeuo pipefail
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
test_case="${1:?name a test case}"
[ "$(uname -s)" = Darwin ] || { echo "macOS only" >&2; exit 2; }
export TEST_ROOT
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT
trap 'printf "FAIL: %s (line %s)\n" "$test_case" "$LINENO"; cat "$TEST_ROOT/output" 2>/dev/null' ERR
mkdir -p "$TEST_ROOT"/{repo/scripts,bin,home}
cp "$REPO_DIR/scripts/"{make-signing-cert.sh,lib.sh} "$TEST_ROOT/repo/scripts/"
for tool in openssl security gh; do
    cp "$REPO_DIR/scripts/tests/make-signing-cert-command.sh" "$TEST_ROOT/bin/$tool"
    chmod 755 "$TEST_ROOT/bin/$tool"
done
export PATH="$TEST_ROOT/bin:$PATH"
export HOME="$TEST_ROOT/home"
export ARIADNE_HOME="$TEST_ROOT/home/.ariadne"
KEY_STORE="$ARIADNE_HOME/signing-cert"

# Seeds a pre-existing certificate: the local key store install.sh's own
# signing step and a later re-export both read, plus the keychain markers
# the security stub answers from. $1 is valid or invalid.
seed_identity() {
    mkdir -p "$KEY_STORE"
    printf 'KEY\n' > "$KEY_STORE/key.pem"
    printf 'CERT\n' > "$KEY_STORE/cert.pem"
    : > "$TEST_ROOT/cert-present"
    [ "$1" = valid ] && : > "$TEST_ROOT/identity-valid"
    return 0
}

run() {
    bash "$TEST_ROOT/repo/scripts/make-signing-cert.sh" "$@" > "$TEST_ROOT/output" 2>&1
}

case "$test_case" in
    create)
        run --export-path "$TEST_ROOT/out.p12"
        grep -E 'OK.*Creating the self-signed certificate' "$TEST_ROOT/output"
        grep -F 'openssl req' "$TEST_ROOT/commands"
        grep -F 'security import' "$TEST_ROOT/commands"
        [ -f "$TEST_ROOT/cert-present" ]
        [ -f "$TEST_ROOT/identity-valid" ]
        [ -f "$KEY_STORE/key.pem" ]
        [ -f "$KEY_STORE/cert.pem" ]
        [ -f "$TEST_ROOT/out.p12" ]
        grep -E 'gh secret set APPLE_CERTIFICATE$' "$TEST_ROOT/output"
        ;;
    idempotent)
        seed_identity valid
        run --export-path "$TEST_ROOT/out.p12"
        if grep -q '^openssl req' "$TEST_ROOT/commands"; then exit 1; fi
        if grep -q '^security import' "$TEST_ROOT/commands"; then exit 1; fi
        if grep -q '^security delete-identity' "$TEST_ROOT/commands"; then exit 1; fi
        grep -E 'OK.*Exporting the existing certificate' "$TEST_ROOT/output"
        [ -f "$TEST_ROOT/out.p12" ]
        ;;
    replace)
        seed_identity valid
        run --force --export-path "$TEST_ROOT/out.p12"
        grep -F 'security delete-identity -t -c Ariadne Code Signing' "$TEST_ROOT/commands"
        grep -F 'openssl req' "$TEST_ROOT/commands"
        grep -E 'OK.*Replacing the existing certificate' "$TEST_ROOT/output"
        [ -f "$TEST_ROOT/identity-valid" ]
        ;;
    repair-invalid)
        # A certificate present but not a valid identity (expired, or never
        # trusted) must still be replaced, without --force: nothing usable
        # exists to keep, and leaving it would import a second certificate
        # of the same name alongside it.
        seed_identity invalid
        run --export-path "$TEST_ROOT/out.p12"
        grep -F 'security delete-identity -t -c Ariadne Code Signing' "$TEST_ROOT/commands"
        grep -F 'openssl req' "$TEST_ROOT/commands"
        grep -E 'OK.*Replacing the existing certificate' "$TEST_ROOT/output"
        [ -f "$TEST_ROOT/identity-valid" ]
        ;;
    export)
        # A quote in the password must reach the .p12 unmangled, and the
        # printed gh secret set command must still be a valid shell command.
        seed_identity valid
        run --export-path "$TEST_ROOT/custom.p12" --password "it's a secret"
        [ -f "$TEST_ROOT/custom.p12" ]
        grep -Fxq "it's a secret" "$TEST_ROOT/custom.p12"
        expected_q="$(printf '%q' "it's a secret")"
        grep -F "gh secret set APPLE_CERTIFICATE_PASSWORD --body $expected_q" "$TEST_ROOT/output"
        ;;
    secrets)
        run --export-path "$TEST_ROOT/out.p12" --set-secrets
        grep -Fx 'gh secret set APPLE_CERTIFICATE' "$TEST_ROOT/commands"
        grep -F 'gh secret set APPLE_CERTIFICATE_PASSWORD' "$TEST_ROOT/commands"
        grep -F 'gh secret set APPLE_SIGNING_IDENTITY --body Ariadne Code Signing' "$TEST_ROOT/commands"
        ;;
    *) exit 2 ;;
esac
printf 'PASS: %s\n' "$test_case"
