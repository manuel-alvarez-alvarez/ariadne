#!/usr/bin/env bash
# Isolated reproduction for SUPPLY-001.
#
# Runs entirely against synthetic, local stand-ins (a local bare git repo and
# locally built wheels) under a scratch directory outside any repository.
# Nothing here touches the real jaredpalmer/kev or NandhaKishorM/laya sources,
# the user's daemon, or any network service.
#
# What it proves, in order, with an explicit assertion for each:
#   1. An unpinned `git+<url>` requirement (the shape bench/ai-permissions/run.sh
#      uses for Kev, and the shape its `latest_laya_wheel` effectively produces
#      for Laya once resolved) installs whatever the remote's default branch
#      points at *at install time* — a second, otherwise identical run after
#      the remote changes installs different content silently.
#   2. The same requirement pinned to a commit SHA keeps installing that one
#      commit's content after the remote changes (what `install.rs`'s
#      `KEV_COMMIT` pin already relies on for Kev in production).
#   3. `pip install --require-hashes` on a direct URL to a wheel (the shape
#      Laya's install could use) accepts the artifact whose sha256 matches the
#      recorded hash, and refuses one that does not.
#   4. The same `--require-hashes` mode refuses a VCS (`git+`) requirement
#      outright, confirming pip's documented limitation
#      (https://pip.pypa.io/en/stable/topics/secure-installs/): Kev's
#      git-sourced install can never be hash-checked, only commit-pinned.
#
# Earlier drafts of this script asserted nothing beyond printing output for a
# human to eyeball, and checked an installed package's content by running
# `python -c "import demo_pkg"` while the shell's current directory was still
# the *source checkout* used to build that same package. Python's `-c` mode
# prepends the current directory to `sys.path`, so that import silently read
# the working tree's `demo_pkg/__init__.py` instead of anything pip actually
# installed into the venv — the checks passed or failed depending on
# incidental `cwd`, never on what pip did. Every check below now runs under
# `python -I` (isolated mode: no `cwd`/script-directory entry on `sys.path`)
# from a directory that holds no package source of its own, and asserts the
# imported module's `__file__` resolves under that venv's own
# `site-packages`, not a repository working tree.
set -euo pipefail

R="$(cd "$(mktemp -d /tmp/supply-001-repro.XXXXXX)" && pwd -P)"
echo "scratch directory: $R"
trap 'echo; echo "(scratch left at $R for inspection; rm -rf it when done)"' EXIT

PY="${PYTHON:-python3}"
NEUTRAL="$R/neutral"   # holds no package source; every content check runs from here
mkdir -p "$NEUTRAL"

say() { printf '\n--- %s ---\n' "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

# Prints the installed module's __file__ and asserts it resolves inside
# `$venv`'s site-packages (not a repository working tree reused by a stale
# `sys.path` entry), then asserts its MARKER equals `$2`.
assert_installed_marker() {
    local venv="$1" module="$2" expected="$3"
    local actual_file actual_marker
    actual_file="$(cd "$NEUTRAL" && "$venv/bin/python" -I -c "import $module; print($module.__file__)")"
    case "$actual_file" in
        "$venv"/*) ;;
        *) fail "$module resolved to $actual_file, not under $venv (sys.path shadowing?)" ;;
    esac
    actual_marker="$(cd "$NEUTRAL" && "$venv/bin/python" -I -c "import $module; print($module.MARKER)")"
    [ "$actual_marker" = "$expected" ] || fail "$module.MARKER was '$actual_marker', expected '$expected'"
    echo "OK: $module.__file__ = $actual_file"
    echo "OK: $module.MARKER = $actual_marker"
}

# Runs `$2..` expecting a non-zero exit whose stderr contains `$1`; fails the
# script otherwise. This is the "test rejection of substituted content"
# assertion — a log that merely shows an error is not, by itself, proof the
# script would have caught a silent success.
assert_refused() {
    local needle="$1"; shift
    local log
    log="$(mktemp)"
    if "$@" >"$log" 2>&1; then
        cat "$log"; fail "expected this command to be refused, but it exited 0: $*"
    fi
    grep -qF "$needle" "$log" || { cat "$log"; fail "refusal did not mention expected text: $needle"; }
    echo "OK: refused as expected, citing: $needle"
    tail -n 4 "$log"
    rm -f "$log"
}

### Part A: unpinned vs pinned VCS resolution ################################
#
# A-unpinned and A-pinned each get their own repo, their own venv, and their
# own PIP_CACHE_DIR — deliberately not sharing state with each other — so
# each demonstrates exactly one claim with nothing left over from a previous
# step to confound the result.

say "A-unpinned, part 1: build a synthetic upstream and install it unpinned"
git init -q -b main --bare "$R/upstream_unpinned.git"
mkdir -p "$R/unpinned_work" && cd "$R/unpinned_work"
git init -q -b main
git config user.email repro@example.com
git config user.name repro
mkdir demo_pkg
printf 'MARKER = "benign-v1"\n' > demo_pkg/__init__.py
cat > pyproject.toml <<'EOF'
[build-system]
requires = ["setuptools>=77"]
build-backend = "setuptools.build_meta"
[project]
name = "demo-pkg"
version = "0.1.0"
EOF
git add -A && git commit -q -m "benign v1"
git remote add origin "$R/upstream_unpinned.git"
git push -q origin main:main

"$PY" -m venv "$R/venv_unpinned_run1"
PIP_CACHE_DIR="$R/cache-unpinned-run1" "$R/venv_unpinned_run1/bin/pip" install -q \
  "demo-pkg @ git+file://$R/upstream_unpinned.git"
echo "this is what run.sh's 'kev[serve] @ git+https://...kev' does (no @<sha>):"
assert_installed_marker "$R/venv_unpinned_run1" demo_pkg benign-v1

say "A-unpinned, part 2: the upstream changes (a maintainer push, or an account takeover, between two runs)"
cd "$R/unpinned_work"
sed -i.bak 's/benign-v1/SWAPPED-v2-malicious/' demo_pkg/__init__.py
git add -A && git commit -q -m "v2"
git push -q origin main:main

"$PY" -m venv "$R/venv_unpinned_run2"
PIP_CACHE_DIR="$R/cache-unpinned-run2" "$R/venv_unpinned_run2/bin/pip" install -q \
  "demo-pkg @ git+file://$R/upstream_unpinned.git"
echo "a second, otherwise identical, unpinned install:"
assert_installed_marker "$R/venv_unpinned_run2" demo_pkg SWAPPED-v2-malicious

say "A-pinned: a separate repo, so the pin never shares state with the unpinned demo above"
git init -q -b main --bare "$R/upstream_pinned.git"
mkdir -p "$R/pinned_work" && cd "$R/pinned_work"
git init -q -b main
git config user.email repro@example.com
git config user.name repro
mkdir demo_pkg2
printf 'MARKER = "pinned-benign-v1"\n' > demo_pkg2/__init__.py
cat > pyproject.toml <<'EOF'
[build-system]
requires = ["setuptools>=77"]
build-backend = "setuptools.build_meta"
[project]
name = "demo-pkg2"
version = "0.1.0"
EOF
git add -A && git commit -q -m "benign v1"
V1_SHA=$(git rev-parse HEAD)
git remote add origin "$R/upstream_pinned.git"
git push -q origin main:main
echo "pinned-demo v1 commit: $V1_SHA"

sed -i.bak 's/pinned-benign-v1/pinned-SWAPPED-v2-malicious/' demo_pkg2/__init__.py
git add -A && git commit -q -m "v2, after the pin was taken"
git push -q origin main:main
echo "pinned-demo v2 commit (the account/upstream changing after the pin): $(git rev-parse HEAD)"

"$PY" -m venv "$R/venv_pinned"
PIP_CACHE_DIR="$R/cache-pinned" "$R/venv_pinned/bin/pip" install -q \
  "demo-pkg2 @ git+file://$R/upstream_pinned.git@$V1_SHA"
echo "pinned install, taken before the v2 push:"
assert_installed_marker "$R/venv_pinned" demo_pkg2 pinned-benign-v1
# Assert, not just print: direct_url.json's recorded commit must be the
# pinned v1 SHA, not whatever HEAD happened to be when pip ran.
(cd "$NEUTRAL" && V1_SHA="$V1_SHA" python3 -I -c "
import json, glob, os, sys
paths = glob.glob('$R/venv_pinned/lib/*/site-packages/demo_pkg2-*.dist-info/direct_url.json')
if not paths:
    sys.exit('FAIL: no direct_url.json found under the venv')
info = json.load(open(paths[0]))['vcs_info']
expected = os.environ['V1_SHA']
if info.get('commit_id') != expected or info.get('requested_revision') != expected:
    sys.exit(f'FAIL: direct_url.json recorded {info!r}, expected commit_id and requested_revision both {expected!r}')
print('OK: direct_url.json vcs_info =', info)
") || fail "direct_url.json did not record the pinned commit"

### Part B: hash-checking catches a substituted release asset #################

say "B1: build a benign wheel and record its sha256 (stand-in for a Laya GitHub release asset)"
mkdir -p "$R/wheel_src/demo_wheel"
printf 'MARKER = "wheel-v1-benign"\n' > "$R/wheel_src/demo_wheel/__init__.py"
cat > "$R/wheel_src/pyproject.toml" <<'EOF'
[build-system]
requires = ["setuptools>=77"]
build-backend = "setuptools.build_meta"
[project]
name = "demo-wheel"
version = "1.0.0"
EOF
"$PY" -m venv "$R/buildenv"
"$R/buildenv/bin/pip" install -q --upgrade build
"$R/buildenv/bin/python" -m build --wheel -o "$R/dist_v1" "$R/wheel_src" > "$R/build_v1.log" 2>&1
WHEEL=$(ls "$R/dist_v1"/*.whl)
HASH_V1=$(python3 -c "import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest())" "$WHEEL")
echo "wheel: $WHEEL"
echo "sha256: $HASH_V1"
printf 'demo-wheel @ file://%s --hash=sha256:%s\n' "$WHEEL" "$HASH_V1" > "$R/requirements.txt"
cat "$R/requirements.txt"

say "B2: install with --require-hashes against the correct artifact (expected: succeeds)"
"$PY" -m venv "$R/venv_hash_ok"
PIP_CACHE_DIR="$R/cache-hash-ok" "$R/venv_hash_ok/bin/pip" install -q --require-hashes -r "$R/requirements.txt"
assert_installed_marker "$R/venv_hash_ok" demo_wheel wheel-v1-benign

say "B3: simulate a compromised/re-published release asset at the SAME URL, same requirements.txt"
mkdir -p "$R/wheel_src_v2/demo_wheel"
printf 'MARKER = "wheel-v2-SWAPPED-malicious"\n' > "$R/wheel_src_v2/demo_wheel/__init__.py"
cp "$R/wheel_src/pyproject.toml" "$R/wheel_src_v2/pyproject.toml"
"$R/buildenv/bin/python" -m build --wheel -o "$R/dist_v2" "$R/wheel_src_v2" > "$R/build_v2.log" 2>&1
cp "$R/dist_v2"/*.whl "$WHEEL"   # overwrite in place, as a re-published release asset would

say "B4: re-install with the OLD recorded hash against the swapped content (expected: pip refuses)"
"$PY" -m venv "$R/venv_hash_reject"
assert_refused "THESE PACKAGES DO NOT MATCH THE HASHES" \
  env PIP_CACHE_DIR="$R/cache-hash-reject" "$R/venv_hash_reject/bin/pip" install --require-hashes -r "$R/requirements.txt"

### Part C: hash-checking is unavailable for VCS requirements #################

say "C1: --require-hashes on a git+ URL (Kev's install shape) is refused outright, pinned or not"
printf 'demo-pkg2 @ git+file://%s@%s --hash=sha256:%s\n' "$R/upstream_pinned.git" "$V1_SHA" \
  "0000000000000000000000000000000000000000000000000000000000000000" > "$R/requirements_vcs.txt"
"$PY" -m venv "$R/venv_vcs_hash"
assert_refused "we don't have a way to hash version control repositories" \
  env PIP_CACHE_DIR="$R/cache-vcs-hash" "$R/venv_vcs_hash/bin/pip" install --require-hashes -r "$R/requirements_vcs.txt"

say "done — every assertion above passed"
echo "This confirms: pin the commit for Kev (hash-checking cannot apply to a VCS source;"
echo "a 40-character SHA, as install.rs already does, is the only integrity control VCS"
echo "requirements support); use a requirements file with --require-hashes and a pinned,"
echo "non-latest Laya wheel URL instead of fetching whatever 'releases/latest' currently"
echo "resolves to."
echo
echo "Scope: this proves pip/git's own resolution and hash-checking behavior against"
echo "synthetic stand-ins. It does not exercise bench/ai-permissions/run.sh itself (its"
echo "venv-selection, --rebuild, or per-backend branches), and it does not install the"
echo "real Laya or Kev package — see the main report's SUPPLY-001 for what is and is not"
echo "claimed from this evidence."
