#!/usr/bin/env bash
# Creates the self-signed "Ariadne Code Signing" certificate scripts/install.sh
# signs local macOS builds with, and the release workflow signs release
# assets with - the same certificate, so every build keys macOS's privacy
# grants (files, microphone, automation) to the same signature instead of an
# ad-hoc one that changes on every build.
#
# A key and a self-signed, code-signing certificate are created with openssl,
# then imported into the login keychain with `security import`, marked usable
# by codesign with no prompt. The certificate is then trusted for code
# signing with `security add-trusted-cert`, which macOS answers with its own
# authentication prompt (Touch ID or a password) the one time a machine does
# this - no Keychain Access wizard is used, but that one system prompt still
# needs a person at the keyboard to approve it.
#
# The key and certificate are also kept at ~/.ariadne/signing-cert, so a
# later run can export them again - a lost .p12 or forgotten password is not
# a reason to replace the identity, which would also change the signature
# every build after carries.
#
# Idempotent: a second run, finding a valid identity already in the login
# keychain, re-exports it rather than replacing it. --force replaces it
# regardless. A certificate present but not a valid identity (expired,
# never trusted) is replaced even without --force, since there is nothing
# usable to keep.
#
# Exports a .p12 holding the certificate and its key, and either prints the
# three `gh secret set` commands the release workflow's secrets are read
# from, or runs them with --set-secrets.
#
# The options are in usage() below, which is what --help prints; they are not
# repeated here so the two cannot drift apart.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/lib.sh
. "$REPO_DIR/scripts/lib.sh"

CERT_CN="Ariadne Code Signing"

# Like run_logged, but never echoes the command line to the log: every
# caller below passes the generated password as a literal argument, and
# run_logged's own echo would otherwise leave it sitting in a log file.
run_quiet() {
    if [ "$UI_VERBOSE" = 1 ]; then
        "$@"
        return $?
    fi
    if [ -n "$UI_LOG" ]; then
        "$@" >> "$UI_LOG" 2>&1
        return $?
    fi
    "$@" > /dev/null 2>&1
    return $?
}

usage() {
    cat <<'EOF'
Ariadne signing certificate - creates the self-signed "Ariadne Code Signing"
identity scripts/install.sh signs local macOS builds with.

Usage: scripts/make-signing-cert.sh [options]

  --force              replace the identity even if a valid one exists
  --export-path FILE   where to write the exported .p12 (default: a temp file)
  --password PASS      the .p12 password (default: a generated one, printed)
  --set-secrets        run `gh secret set` for APPLE_CERTIFICATE,
                        APPLE_CERTIFICATE_PASSWORD and APPLE_SIGNING_IDENTITY
                        instead of printing the three commands (default)
  --verbose             stream subcommand output instead of capturing it
  --quiet               print errors and the final summary only
  --dry-run             print the steps that would run, change nothing
  --yes, -y             accepted for symmetry; nothing here asks
  --help, -h            show this help

Darwin only. A second run, finding a valid identity already there, exports
it again rather than replacing it - a lost .p12 or forgotten password is not
a reason to change the signature every build carries. Run it again with
--force, and set the secrets again, only if the certificate itself is lost
(its login keychain entry removed, or ~/.ariadne/signing-cert deleted).

macOS asks for your approval, with its own authentication prompt, the first
time a certificate is trusted for code signing on this machine - that one
prompt is not this script, and nothing here can answer it for you.
EOF
}

FORCE=0
EXPORT_PATH=""
PASSWORD=""
SET_SECRETS=0
while [ $# -gt 0 ]; do
    if ui_common_flag "$1"; then shift; continue; fi
    case "$1" in
        --force) FORCE=1; shift ;;
        --export-path) EXPORT_PATH="$2"; shift 2 ;;
        --password) PASSWORD="$2"; shift 2 ;;
        --set-secrets) SET_SECRETS=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; echo >&2; usage >&2; exit 2 ;;
    esac
done

ui_init
trap 'ui_on_err $?' ERR
ui_locations
LOGIN_KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
CERT_STORE_DIR="$ARIADNE_HOME/signing-cert"
KEY_FILE="$CERT_STORE_DIR/key.pem"
CERT_FILE="$CERT_STORE_DIR/cert.pem"

[ "$OS" = Darwin ] \
    || ui_die "$CERT_CN signs macOS builds and only runs on Darwin - nothing to do on $OS"

# Read-only, and cheap enough to run before the plan - like install.sh's own
# detect_target - so --dry-run can show accurately what this run will do.
# CERT_PRESENT is any certificate by this name, valid or not: a certificate
# install left behind without a trusted or current key must still be found
# and cleared before a new one is imported, or the keychain ends up holding
# two certificates of the same name. VALID is a codesign-usable identity.
# -p: without it, find-certificate exits 0 whether or not anything actually
# matched, which is useless as a presence check.
CERT_PRESENT=0
security find-certificate -c "$CERT_CN" -p "$LOGIN_KEYCHAIN" > /dev/null 2>&1 && CERT_PRESENT=1
VALID=0
security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$CERT_CN\"" && VALID=1

REPLACE=1
[ "$VALID" = 1 ] && [ "$FORCE" = 0 ] && REPLACE=0

LOG_FILE="${TMPDIR:-/tmp}/ariadne-signing-cert.log"

# --- the plan ------------------------------------------------------------------
if [ "$REPLACE" = 1 ]; then
    if [ "$CERT_PRESENT" = 1 ]; then
        plan_add "Replacing the existing certificate"
    else
        plan_add "Creating the self-signed certificate"
    fi
    plan_add "Importing into the login keychain"
else
    plan_add "Exporting the existing certificate"
fi
if [ "$SET_SECRETS" = 1 ]; then
    plan_add "Setting the repository secrets"
else
    plan_add "Printing the repository secrets"
fi
ui_start

ui_header "Ariadne signing certificate" \
    "identity $CERT_CN" \
    "export   $([ -n "$EXPORT_PATH" ] && ui_tilde "$EXPORT_PATH" || printf 'a temporary file')" \
    "log      $(ui_tilde "$LOG_FILE")"

if [ "$UI_DRY_RUN" = 1 ]; then
    plan_print
    exit 0
fi

ui_log_init "$LOG_FILE"

# Resolved only now: --dry-run must change nothing, and a generated password
# or temp path is as much a change as a file would be.
[ -n "$EXPORT_PATH" ] || EXPORT_PATH="$(mktemp "${TMPDIR:-/tmp}/ariadne-code-signing.XXXXXX")"
[ -n "$PASSWORD" ] || PASSWORD="$(openssl rand -base64 24)"
mkdir -p "$(dirname "$EXPORT_PATH")"

# Exports $KEY_FILE/$CERT_FILE to $EXPORT_PATH under $PASSWORD, with no
# secret ever reaching run_logged's own command-line echo.
export_p12() {
    local pass_dir status
    pass_dir="$(mktemp -d "${TMPDIR:-/tmp}/ariadne-signing-cert.XXXXXX")"
    printf '%s' "$PASSWORD" > "$pass_dir/password"
    chmod 600 "$pass_dir/password"
    # certpbe/keypbe/macalg name the algorithms macOS's Security framework
    # can read back out of a PKCS#12 file: OpenSSL 3's own default (AES-256,
    # a SHA-256 MAC) imports as a MAC verification failure on macOS.
    run_quiet openssl pkcs12 -export -inkey "$KEY_FILE" -in "$CERT_FILE" -out "$EXPORT_PATH" \
        -passout "file:$pass_dir/password" -name "$CERT_CN" \
        -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg SHA1
    status=$?
    rm -rf "$pass_dir"
    return $status
}

if [ "$REPLACE" = 1 ]; then
    # --- create / replace ----------------------------------------------------------
    step_begin
    if [ "$CERT_PRESENT" = 1 ]; then
        # delete-identity removes a certificate with its matching key;
        # delete-certificate catches a bare certificate install left behind
        # with no key of its own (delete-identity would otherwise find no
        # match and silently leave it in place).
        run_logged security delete-identity -t -c "$CERT_CN" "$LOGIN_KEYCHAIN" 2>/dev/null
        if security find-certificate -c "$CERT_CN" -p "$LOGIN_KEYCHAIN" > /dev/null 2>&1; then
            run_logged security delete-certificate -t -c "$CERT_CN" "$LOGIN_KEYCHAIN" \
                || ui_die "security could not delete the existing \"$CERT_CN\" certificate"
        fi
    fi
    mkdir -p "$CERT_STORE_DIR"
    chmod 700 "$CERT_STORE_DIR"
    run_logged openssl req -x509 -newkey rsa:2048 -keyout "$KEY_FILE" -out "$CERT_FILE" \
        -days 3650 -nodes -subj "/CN=$CERT_CN" \
        -addext "basicConstraints=critical,CA:true" \
        -addext "keyUsage=critical,digitalSignature" \
        -addext "extendedKeyUsage=critical,codeSigning" \
        -addext "subjectKeyIdentifier=hash" \
        -addext "authorityKeyIdentifier=keyid:always" \
        || ui_die "openssl could not create the certificate"
    chmod 600 "$KEY_FILE" "$CERT_FILE"
    export_p12 || ui_die "openssl could not export the .p12"
    chmod 600 "$EXPORT_PATH"
    step_ok "$(ui_tilde "$EXPORT_PATH")"

    # --- import and trust --------------------------------------------------------------
    step_begin
    # -f pkcs12: security otherwise guesses the format from the file's
    # extension, and a --export-path or temp file with no .p12 suffix is
    # read as "Unknown format" without it.
    run_quiet security import "$EXPORT_PATH" -f pkcs12 -k "$LOGIN_KEYCHAIN" -P "$PASSWORD" -T /usr/bin/codesign \
        || ui_die "security could not import the certificate"
    # Trusting a certificate for code signing is a per-user Trust Settings
    # change, which macOS always gates behind its own authentication prompt
    # (Touch ID or a password) - security add-trusted-cert triggers it
    # rather than answering it, so a declined or unapproved prompt leaves
    # the identity imported but still invalid, caught by the check below.
    run_logged security add-trusted-cert -p codeSign -k "$LOGIN_KEYCHAIN" "$CERT_FILE" || true
    security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$CERT_CN\"" \
        || ui_die "macOS does not list \"$CERT_CN\" as a valid identity yet - approve the authentication prompt it showed for trusting the certificate, then re-run"
    step_ok
else
    # --- re-export the existing identity ------------------------------------------------
    step_begin
    [ -f "$KEY_FILE" ] && [ -f "$CERT_FILE" ] \
        || ui_die "no local copy of \"$CERT_CN\"'s key at $(ui_tilde "$CERT_STORE_DIR") - re-run with --force to recreate it"
    export_p12 || ui_die "openssl could not export the .p12"
    chmod 600 "$EXPORT_PATH"
    step_ok "$(ui_tilde "$EXPORT_PATH")"
fi

# --- secrets -----------------------------------------------------------------------
step_begin
if [ "$SET_SECRETS" = 1 ]; then
    command -v gh > /dev/null 2>&1 \
        || ui_die "the GitHub CLI (gh) is required for --set-secrets - install it from https://cli.github.com"
    # shellcheck disable=SC2016
    run_logged bash -c 'base64 -i "$1" | gh secret set APPLE_CERTIFICATE' _ "$EXPORT_PATH" \
        || ui_die "gh secret set APPLE_CERTIFICATE failed"
    run_quiet gh secret set APPLE_CERTIFICATE_PASSWORD --body "$PASSWORD" \
        || ui_die "gh secret set APPLE_CERTIFICATE_PASSWORD failed"
    run_logged gh secret set APPLE_SIGNING_IDENTITY --body "$CERT_CN" \
        || ui_die "gh secret set APPLE_SIGNING_IDENTITY failed"
    step_ok "APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY"
else
    step_ok
fi

# --- summary ----------------------------------------------------------------------
printf '\n%s%s is in your login keychain.%s\n\n' "$UI_B$UI_GREEN" "$CERT_CN" "$UI_R"
ui_field "export" "$(ui_tilde "$EXPORT_PATH")"
ui_field "password" "$PASSWORD"
if [ "$SET_SECRETS" = 1 ]; then
    ui_field "secrets" "set on the repository in this checkout's origin remote"
else
    printf '\n%sSet the three secrets the release workflow signs with:%s\n\n' "$UI_B" "$UI_R"
    printf '  base64 -i %s | gh secret set APPLE_CERTIFICATE\n' "$(printf '%q' "$EXPORT_PATH")"
    printf '  gh secret set APPLE_CERTIFICATE_PASSWORD --body %s\n' "$(printf '%q' "$PASSWORD")"
    printf '  gh secret set APPLE_SIGNING_IDENTITY --body %s\n' "$(printf '%q' "$CERT_CN")"
fi
printf '\n  %sIf the certificate itself is ever lost, run this again with --force and set the secrets again.%s\n\n' "$UI_D" "$UI_R"
