#!/usr/bin/env bash
# Isolated reproduction for SUPPLY-001.
#
# Runs entirely against synthetic, local stand-ins (a local bare git repo and
# locally built wheels) under a scratch directory outside any repository.
# Nothing here touches the real jaredpalmer/kev or NandhaKishorM/laya sources,
# the user's daemon, or any network service.
#
# What it proves, in order:
#   1. An unpinned `git+<url>` requirement (the shape bench/ai-permissions/run.sh
#      uses for Kev, and the shape its `latest_laya_wheel` effectively produces
#      for Laya once resolved) installs whatever the remote's default branch
#      points at *at install time* — a second, otherwise identical run after
#      the remote changes installs different content silently.
#   2. The same requirement pinned to a commit SHA keeps installing that one
#      commit's content after the remote changes (this is what
#      crates/ariadne-daemon/src/ai_permissions/install.rs already does for
#      Kev, and what bench/ai-permissions/run.sh does not do for either
#      dependency).
#   3. `pip install --require-hashes` on a direct URL to a wheel (the shape
#      Laya's install could use) accepts the artifact whose sha256 matches the
#      recorded hash, and refuses one that does not — i.e. hash-checking is
#      available and would have caught a substituted release asset.
#   4. The same `--require-hashes` mode explicitly refuses a VCS (`git+`)
#      requirement, confirming pip's own documented limitation
#      (https://pip.pypa.io/en/stable/topics/secure-installs/): Kev's
#      git-sourced install can never be hash-checked, only commit-pinned.
#
# Usage: bash supply-001-pip-integrity.sh
# Requires: python3 with venv and pip, git, the `build` package (installed
# into a throwaway venv by this script). Network access is not required
# beyond what pip needs to fetch `build`'s own dependencies the first time.
set -euo pipefail

R="$(mktemp -d /tmp/supply-001-repro.XXXXXX)"
echo "scratch directory: $R"
trap 'echo; echo "(scratch left at $R for inspection; rm -rf it when done)"' EXIT

PY="${PYTHON:-python3}"

# Every pip invocation below gets its own PIP_CACHE_DIR under $R (set at the
# call site, not exported once), and every scenario gets its own throwaway
# git repo or wheel directory. This is deliberate, not cosmetic: while
# drafting this script, chaining several pip installs of the same package
# name/version against evolving shared fixtures, under one shared cache,
# produced an install that silently reused a previous step's build instead of
# the one its own command line asked for. That is a pip build-cache
# interaction specific to reusing one name+version across steps in one
# session — not a property of Kev's or Laya's real install (each of Ariadne's
# own installs is the only thing touching its own fresh venv) and not this
# finding's claim. Giving each scenario below its own repo, venv and cache
# avoids re-triggering it and keeps each result attributable to exactly the
# one thing that scenario changes.

say() { printf '\n--- %s ---\n' "$*"; }

### Part A: unpinned vs pinned VCS resolution ################################
#
# A-unpinned and A-pinned each get their own repo, their own venv, and their
# own PIP_CACHE_DIR, deliberately not sharing any state — including with each
# other — so each demonstrates exactly one claim with nothing left over from
# a previous step to confound the result.

say "A-unpinned, part 1: build a synthetic upstream and install it unpinned"
git init -q --bare "$R/upstream_unpinned.git"
mkdir -p "$R/unpinned_work" && cd "$R/unpinned_work"
git init -q
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
git push -q origin HEAD:main

"$PY" -m venv "$R/venv_unpinned_run1"
PIP_CACHE_DIR="$R/cache-unpinned-run1" "$R/venv_unpinned_run1/bin/pip" install -q \
  "demo-pkg @ git+file://$R/upstream_unpinned.git"
echo -n "this is what run.sh's 'kev[serve] @ git+https://...kev' does (no @<sha>) -- run1 installed marker: "
"$R/venv_unpinned_run1/bin/python" -c "import demo_pkg; print(demo_pkg.MARKER)"

say "A-unpinned, part 2: the upstream changes (a maintainer push, or an account takeover, between two runs)"
cd "$R/unpinned_work"
sed -i.bak 's/benign-v1/SWAPPED-v2-malicious/' demo_pkg/__init__.py
git add -A && git commit -q -m "v2"
git push -q origin HEAD:main

"$PY" -m venv "$R/venv_unpinned_run2"
PIP_CACHE_DIR="$R/cache-unpinned-run2" "$R/venv_unpinned_run2/bin/pip" install -q \
  "demo-pkg @ git+file://$R/upstream_unpinned.git"
echo -n "a second, otherwise identical, unpinned install -- run2 installed marker: "
"$R/venv_unpinned_run2/bin/python" -c "import demo_pkg; print(demo_pkg.MARKER)"

say "A-pinned: a separate repo, so the pin never shares state with the unpinned demo above"
git init -q --bare "$R/upstream_pinned.git"
mkdir -p "$R/pinned_work" && cd "$R/pinned_work"
git init -q
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
git push -q origin HEAD:main
echo "pinned-demo v1 commit: $V1_SHA"

sed -i.bak 's/pinned-benign-v1/pinned-SWAPPED-v2-malicious/' demo_pkg2/__init__.py
git add -A && git commit -q -m "v2, after the pin was taken"
git push -q origin HEAD:main
echo "pinned-demo v2 commit (the account/upstream changing after the pin): $(git rev-parse HEAD)"

"$PY" -m venv "$R/venv_pinned"
PIP_CACHE_DIR="$R/cache-pinned" "$R/venv_pinned/bin/pip" install -q \
  "demo-pkg2 @ git+file://$R/upstream_pinned.git@$V1_SHA"
echo -n "pinned install, taken before the v2 push, installed marker: "
"$R/venv_pinned/bin/python" -c "import demo_pkg2; print(demo_pkg2.MARKER)"
echo -n "pinned install, recorded commit (direct_url.json): "
python3 -c "
import json, glob
for p in glob.glob('$R/venv_pinned/lib/*/site-packages/demo_pkg2-*.dist-info/direct_url.json'):
    print(json.load(open(p))['vcs_info'])
"

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
echo -n "installed marker: "
"$R/venv_hash_ok/bin/python" -c "import demo_wheel; print(demo_wheel.MARKER)"

say "B3: simulate a compromised/re-published release asset at the SAME URL, same requirements.txt"
mkdir -p "$R/wheel_src_v2/demo_wheel"
printf 'MARKER = "wheel-v2-SWAPPED-malicious"\n' > "$R/wheel_src_v2/demo_wheel/__init__.py"
cp "$R/wheel_src/pyproject.toml" "$R/wheel_src_v2/pyproject.toml"
"$R/buildenv/bin/python" -m build --wheel -o "$R/dist_v2" "$R/wheel_src_v2" > "$R/build_v2.log" 2>&1
cp "$R/dist_v2"/*.whl "$WHEEL"   # overwrite in place, as a re-published release asset would

say "B4: re-install with the OLD recorded hash against the swapped content (expected: pip refuses)"
"$PY" -m venv "$R/venv_hash_reject"
set +e
PIP_CACHE_DIR="$R/cache-hash-reject" "$R/venv_hash_reject/bin/pip" install --require-hashes -r "$R/requirements.txt" > "$R/reject.log" 2>&1
STATUS=$?
set -e
echo "exit status: $STATUS"
tail -n 6 "$R/reject.log"

### Part C: hash-checking is unavailable for VCS requirements #################

say "C1: --require-hashes on a git+ URL (Kev's install shape) is refused outright, pinned or not"
printf 'demo-pkg2 @ git+file://%s@%s --hash=sha256:%s\n' "$R/upstream_pinned.git" "$V1_SHA" "0000000000000000000000000000000000000000000000000000000000000000" > "$R/requirements_vcs.txt"
"$PY" -m venv "$R/venv_vcs_hash"
set +e
PIP_CACHE_DIR="$R/cache-vcs-hash" "$R/venv_vcs_hash/bin/pip" install --require-hashes -r "$R/requirements_vcs.txt" > "$R/vcs_hash.log" 2>&1
STATUS=$?
set -e
echo "exit status: $STATUS"
cat "$R/vcs_hash.log" | tail -n 4

say "done"
echo "This confirms: pin the commit for Kev (hash-checking cannot apply to a VCS source;"
echo "a 40-character SHA, as install.rs already does, is the only integrity control VCS"
echo "requirements support); add --hash to a pinned, non-latest Laya wheel URL instead of"
echo "fetching whatever 'releases/latest' currently resolves to."
