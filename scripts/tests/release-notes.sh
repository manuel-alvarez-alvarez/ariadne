#!/usr/bin/env bash
# Run on any OS: bash scripts/tests/release-notes.sh CASE, for one of its
# cases: first, rerun, two-targets, crlf. Proves scripts/release-notes.sh's
# own behavior against a throwaway git repository (a real `origin` remote
# and real tags - no stand-in needed, since the script shells out to
# nothing but git, grep, awk, sed, tr and basename).
set -Eeuo pipefail
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
test_case="${1:?name a test case}"
export TEST_ROOT
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT
trap 'printf "FAIL: %s (line %s)\n" "$test_case" "$LINENO"; cat "$TEST_ROOT/out1" "$TEST_ROOT/out2" 2>/dev/null' ERR

cd "$TEST_ROOT"
git init -q
git -c tag.gpgsign=false remote add origin git@github.com:acme/widget.git
git commit --allow-empty -q -m init
git -c tag.gpgsign=false tag v0.1.0
git commit --allow-empty -q -m second
git -c tag.gpgsign=false tag v0.2.0

run() {
    bash "$REPO_DIR/scripts/release-notes.sh" "$@"
}

case "$test_case" in
    first)
        # A plain changelog body, no markers yet: the generated header and
        # table are prepended, wrapped once, and the previous tag comes
        # from git since the body names none.
        printf '### Features\n\n* add the thing\n' > body.txt
        printf 'abc111  ariadne-aarch64-apple-darwin.tar.gz\n' > checksums-aarch64-apple-darwin.txt
        run v0.2.0 body.txt checksums-aarch64-apple-darwin.txt > out1
        grep -Fxc '<!-- ariadne-release-notes:start -->' out1 | grep -Fxq 1
        grep -Fxc '<!-- ariadne-release-notes:end -->' out1 | grep -Fxq 1
        grep -Fxc '## Install' out1 | grep -Fxq 1
        grep -Fxc '| Asset | Target | SHA-256 |' out1 | grep -Fxq 1
        grep -F 'scripts/install.sh --version v0.2.0' out1
        grep -F '[ariadne-aarch64-apple-darwin.tar.gz](https://github.com/acme/widget/releases/download/v0.2.0/ariadne-aarch64-apple-darwin.tar.gz)' out1
        grep -F '| aarch64-apple-darwin | abc111 |' out1
        grep -F '**Full Changelog**: https://github.com/acme/widget/compare/v0.1.0...v0.2.0' out1
        grep -Fxq '### Features' out1
        grep -Fxq '* add the thing' out1
        ;;
    rerun)
        # Running the script on its own output must change nothing: the
        # generated part stays the same single block, and the
        # release-please notes after it are carried unchanged, even with no
        # git tag to fall back on this time.
        printf '### Features\n\n* add the thing\n' > body.txt
        printf 'abc111  ariadne-aarch64-apple-darwin.tar.gz\n' > checksums-aarch64-apple-darwin.txt
        run v0.2.0 body.txt checksums-aarch64-apple-darwin.txt > out1
        git tag -d v0.1.0 v0.2.0 > /dev/null
        run v0.2.0 out1 checksums-aarch64-apple-darwin.txt > out2
        diff out1 out2
        grep -Fxc '<!-- ariadne-release-notes:start -->' out2 | grep -Fxq 1
        grep -Fxc '<!-- ariadne-release-notes:end -->' out2 | grep -Fxq 1
        grep -Fxc '## Install' out2 | grep -Fxq 1
        grep -Fxc '| Asset | Target | SHA-256 |' out2 | grep -Fxq 1
        grep -F '**Full Changelog**: https://github.com/acme/widget/compare/v0.1.0...v0.2.0' out2
        ;;
    two-targets)
        # One checksums file per target: every line of every file becomes
        # its own row, against one header and one table.
        printf '### Fixes\n\n* fix the thing\n' > body.txt
        printf 'abc111  ariadne-aarch64-apple-darwin.tar.gz\ndef222  ariadne-desktop-aarch64-apple-darwin.app.tar.gz\n' \
            > checksums-aarch64-apple-darwin.txt
        printf 'xyz333  ariadne-x86_64-unknown-linux-gnu.tar.gz\n' \
            > checksums-x86_64-unknown-linux-gnu.txt
        run v0.2.0 body.txt checksums-aarch64-apple-darwin.txt checksums-x86_64-unknown-linux-gnu.txt > out1
        grep -Fxc '## Install' out1 | grep -Fxq 1
        grep -Fxc '| Asset | Target | SHA-256 |' out1 | grep -Fxq 1
        [ "$(grep -Fc '| aarch64-apple-darwin |' out1)" = 2 ]
        [ "$(grep -Fc '| x86_64-unknown-linux-gnu |' out1)" = 1 ]
        grep -F '| [ariadne-aarch64-apple-darwin.tar.gz](https://github.com/acme/widget/releases/download/v0.2.0/ariadne-aarch64-apple-darwin.tar.gz) | aarch64-apple-darwin | abc111 |' out1
        grep -F '| [ariadne-desktop-aarch64-apple-darwin.app.tar.gz](https://github.com/acme/widget/releases/download/v0.2.0/ariadne-desktop-aarch64-apple-darwin.app.tar.gz) | aarch64-apple-darwin | def222 |' out1
        grep -F '| [ariadne-x86_64-unknown-linux-gnu.tar.gz](https://github.com/acme/widget/releases/download/v0.2.0/ariadne-x86_64-unknown-linux-gnu.tar.gz) | x86_64-unknown-linux-gnu | xyz333 |' out1
        ;;
    crlf)
        # A body edited in the GitHub web UI comes back with \r\n line
        # ends. A re-run on one must still find the markers and keep the
        # release-please notes, rather than reading the whole body as
        # unmarked and dropping them.
        printf '### Features\n\n* add the thing\n' > body.txt
        printf 'abc111  ariadne-aarch64-apple-darwin.tar.gz\n' > checksums-aarch64-apple-darwin.txt
        run v0.2.0 body.txt checksums-aarch64-apple-darwin.txt > out1
        sed 's/$/\r/' out1 > out1.crlf
        run v0.2.0 out1.crlf checksums-aarch64-apple-darwin.txt > out2
        grep -Fxc '<!-- ariadne-release-notes:start -->' out2 | grep -Fxq 1
        grep -Fxc '<!-- ariadne-release-notes:end -->' out2 | grep -Fxq 1
        grep -Fxc '## Install' out2 | grep -Fxq 1
        grep -Fxc '| Asset | Target | SHA-256 |' out2 | grep -Fxq 1
        grep -Fxq '### Features' out2
        grep -Fxq '* add the thing' out2
        ;;
    *) exit 2 ;;
esac
printf 'PASS: %s\n' "$test_case"
