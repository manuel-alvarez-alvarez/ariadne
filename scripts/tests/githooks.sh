#!/usr/bin/env bash
# Run as: bash scripts/tests/githooks.sh CASE, for one of the cases below.
# Builds a throwaway $TEST_ROOT with a git repository copy and a fake PATH
# standing in for cargo, npm, npx and shellcheck, then drives the hooks
# under .githooks/ directly.
set -Eeuo pipefail
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
test_case="${1:?name a test case}"
export TEST_ROOT
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT
trap 'printf "FAIL: %s (line %s)\n" "$test_case" "$LINENO"; cat "$TEST_ROOT/output" 2>/dev/null' ERR

mkdir -p "$TEST_ROOT/bin"
for tool in cargo npm npx shellcheck; do
    cp "$REPO_DIR/scripts/tests/githooks-command.sh" "$TEST_ROOT/bin/$tool"
    chmod 755 "$TEST_ROOT/bin/$tool"
done
export PATH="$TEST_ROOT/bin:$PATH"

# A synthetic repository with one file per area, and the hooks under test
# copied in from the real repository.
setup_repo() {
    mkdir -p "$TEST_ROOT/repo"/{.githooks,scripts/tests,crates/ariadne-core/src,ui/src,ui/src-tauri,docs,ui/node_modules}
    git -C "$TEST_ROOT/repo" init -q -b main
    git -C "$TEST_ROOT/repo" config user.email test@example.com
    git -C "$TEST_ROOT/repo" config user.name "Test"
    cp "$REPO_DIR/.githooks/pre-commit" "$REPO_DIR/.githooks/pre-push" \
        "$REPO_DIR/.githooks/commit-msg" "$REPO_DIR/.githooks/lib.sh" \
        "$TEST_ROOT/repo/.githooks/"
    chmod 755 "$TEST_ROOT/repo/.githooks/"*
    cp "$REPO_DIR/scripts/install-githooks.sh" "$TEST_ROOT/repo/scripts/"
    cp "$REPO_DIR/scripts/tests/release-please-config.sh" "$TEST_ROOT/repo/scripts/tests/"
    chmod 755 "$TEST_ROOT/repo/scripts/install-githooks.sh"
    cp "$REPO_DIR/scripts/tests/githooks-command.sh" "$TEST_ROOT/repo/scripts/check-unused-rust"
    chmod 755 "$TEST_ROOT/repo/scripts/check-unused-rust"
    printf 'pub fn seeded() {}\n' > "$TEST_ROOT/repo/crates/ariadne-core/src/lib.rs"
    printf 'export const seeded = 1\n' > "$TEST_ROOT/repo/ui/src/seeded.ts"
    printf 'fn seeded() {}\n' > "$TEST_ROOT/repo/ui/src-tauri/seeded.rs"
    printf '# seeded\n' > "$TEST_ROOT/repo/docs/seeded.md"
    cat > "$TEST_ROOT/repo/release-please-config.json" <<'EOF'
{"changelog-sections":[{"type":"perf","section":"⚡ Performance","hidden":false},{"type":"ci","section":"Continuous integration","hidden":true}]}
EOF
    git -C "$TEST_ROOT/repo" add -A
    git -C "$TEST_ROOT/repo" commit -q -m "chore: seed repository"
}

setup_foreign_repo() {
    mkdir "$TEST_ROOT/foreign"
    git -C "$TEST_ROOT/foreign" init -q -b main
    git -C "$TEST_ROOT/foreign" config user.email test@example.com
    git -C "$TEST_ROOT/foreign" config user.name "Test"
    printf 'foreign\n' > "$TEST_ROOT/foreign/file"
    git -C "$TEST_ROOT/foreign" add file
    git -C "$TEST_ROOT/foreign" commit -q -m "chore: seed foreign repository"
}

# Stage one change and run pre-commit, with the command log reset first.
run_pre_commit_on() {
    printf '\n// changed\n' >> "$TEST_ROOT/repo/$1"
    git -C "$TEST_ROOT/repo" add "$1"
    rm -f "$TEST_ROOT/commands"
    if "$TEST_ROOT/repo/.githooks/pre-commit" > "$TEST_ROOT/output" 2>&1; then
        pre_commit_status=0
    else
        pre_commit_status=$?
    fi
}

no_command_matching() {
    ! grep -F "$1" "$TEST_ROOT/commands" > /dev/null 2>&1
}

case "$test_case" in
    install)
        setup_repo
        bash "$TEST_ROOT/repo/scripts/install-githooks.sh" > "$TEST_ROOT/output" 2>&1
        [ "$(git -C "$TEST_ROOT/repo" config --get core.hooksPath)" = ".githooks" ]
        git -C "$TEST_ROOT/repo" worktree add -q "$TEST_ROOT/wt" -b wt-branch
        [ "$(git -C "$TEST_ROOT/wt" config --get core.hooksPath)" = ".githooks" ]
        printf '\n// linked worktree\n' >> "$TEST_ROOT/wt/ui/src/seeded.ts"
        git -C "$TEST_ROOT/wt" add ui/src/seeded.ts
        rm -f "$TEST_ROOT/commands"
        git -C "$TEST_ROOT/wt" commit -q -m "feat(ui): check a linked worktree"
        grep -Fx 'npx biome check src/seeded.ts' "$TEST_ROOT/commands"
        grep -Fx 'npm run typecheck' "$TEST_ROOT/commands"
        bash "$TEST_ROOT/repo/scripts/install-githooks.sh" > "$TEST_ROOT/output2" 2>&1
        grep -F 'already .githooks' "$TEST_ROOT/output2"
        ;;
    rust-commit)
        setup_repo
        run_pre_commit_on crates/ariadne-core/src/lib.rs
        [ "$pre_commit_status" = 0 ]
        grep -Fx 'cargo fmt --all -- --check' "$TEST_ROOT/commands"
        grep -Fx 'cargo clippy -p ariadne-core --all-targets -- -D warnings' "$TEST_ROOT/commands"
        no_command_matching npm
        ;;
    ui-commit)
        setup_repo
        run_pre_commit_on ui/src/seeded.ts
        [ "$pre_commit_status" = 0 ]
        grep -Fx 'npx biome check src/seeded.ts' "$TEST_ROOT/commands"
        grep -Fx 'npm run typecheck' "$TEST_ROOT/commands"
        no_command_matching cargo
        ;;
    unicode-commit)
        setup_repo
        printf 'export const seeded = 2\n' > "$TEST_ROOT/repo/ui/src/café.ts"
        git -C "$TEST_ROOT/repo" add "ui/src/café.ts"
        rm -f "$TEST_ROOT/commands"
        if "$TEST_ROOT/repo/.githooks/pre-commit" > "$TEST_ROOT/output" 2>&1; then
            pre_commit_status=0
        else
            pre_commit_status=$?
        fi
        [ "$pre_commit_status" = 0 ]
        grep -Fx 'npx biome check src/café.ts' "$TEST_ROOT/commands"
        no_command_matching cargo
        ;;
    tauri-commit)
        setup_repo
        run_pre_commit_on ui/src-tauri/seeded.rs
        [ "$pre_commit_status" = 0 ]
        grep -Fx 'cargo fmt --all -- --check' "$TEST_ROOT/commands"
        no_command_matching npm
        no_command_matching clippy
        ;;
    docs-commit)
        setup_repo
        run_pre_commit_on docs/seeded.md
        [ "$pre_commit_status" = 0 ]
        [ ! -s "$TEST_ROOT/commands" ]
        ;;
    release-please-config-commit)
        setup_repo
        printf '\n' >> "$TEST_ROOT/repo/release-please-config.json"
        git -C "$TEST_ROOT/repo" add release-please-config.json
        rm -f "$TEST_ROOT/commands"
        "$TEST_ROOT/repo/.githooks/pre-commit" > "$TEST_ROOT/output" 2>&1
        grep -Fx '+ bash scripts/tests/release-please-config.sh' "$TEST_ROOT/output"
        grep -Fx 'PASS: release-please-config' "$TEST_ROOT/output"
        ;;
    bad-subject)
        setup_repo
        printf 'Added a thing\n' > "$TEST_ROOT/bad-msg"
        printf 'feat(daemon): add a thing\n' > "$TEST_ROOT/good-msg"
        if "$TEST_ROOT/repo/.githooks/commit-msg" "$TEST_ROOT/bad-msg" > "$TEST_ROOT/output" 2>&1; then
            exit 1
        fi
        "$TEST_ROOT/repo/.githooks/commit-msg" "$TEST_ROOT/good-msg"
        ;;
    rust-push)
        setup_repo
        base="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        printf '\n// changed\n' >> "$TEST_ROOT/repo/crates/ariadne-core/src/lib.rs"
        git -C "$TEST_ROOT/repo" add -A
        git -C "$TEST_ROOT/repo" commit -q -m "feat(core): change"
        head="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        rm -f "$TEST_ROOT/commands"
        printf 'refs/heads/main %s refs/heads/main %s\n' "$head" "$base" \
            | "$TEST_ROOT/repo/.githooks/pre-push" origin > "$TEST_ROOT/output" 2>&1
        for cmd in 'cargo machete .' \
            'cargo nextest run --workspace --profile ci' 'cargo test --doc --workspace' \
            'cargo clippy --workspace --all-targets -- -D warnings' 'cargo fmt --all -- --check' \
            'check-unused-rust'; do
            grep -Fx "$cmd" "$TEST_ROOT/commands"
        done
        no_command_matching npm
        ;;
    push-isolation)
        setup_repo
        git init --bare -q "$TEST_ROOT/remote.git"
        git -C "$TEST_ROOT/repo" remote add origin "$TEST_ROOT/remote.git"
        bash "$TEST_ROOT/repo/scripts/install-githooks.sh" > "$TEST_ROOT/output" 2>&1
        git -C "$TEST_ROOT/repo" push -q -u origin main
        git -C "$TEST_ROOT/repo" worktree add -q "$TEST_ROOT/wt" -b wt-branch
        printf '\n// pushed change\n' >> "$TEST_ROOT/wt/crates/ariadne-core/src/lib.rs"
        git -C "$TEST_ROOT/wt" add crates/ariadne-core/src/lib.rs
        setup_foreign_repo
        TEST_GIT_PROBE=1 git -C "$TEST_ROOT/wt" commit -q -m "feat(core): check push isolation"
        [ "$(git -C "$TEST_ROOT/wt" branch --show-current)" = wt-branch ]
        ! git -C "$TEST_ROOT/wt" for-each-ref --format='%(refname)' refs/heads \
            | grep -Fx refs/heads/githooks-probe > /dev/null
        [ -z "$(git -C "$TEST_ROOT/wt" status --porcelain)" ]
        grep -F '// pushed change' "$TEST_ROOT/wt/crates/ariadne-core/src/lib.rs"
        caller_head="$(git -C "$TEST_ROOT/wt" rev-parse HEAD)"
        caller_branch="$(git -C "$TEST_ROOT/wt" branch --show-current)"
        caller_refs="$(git -C "$TEST_ROOT/wt" for-each-ref refs/heads)"
        caller_index="$(git -C "$TEST_ROOT/wt" write-tree)"
        caller_status="$(git -C "$TEST_ROOT/wt" status --porcelain)"
        rm -f "$TEST_ROOT/commands"
        TEST_GIT_PROBE=1 git -C "$TEST_ROOT/wt" push -q -u origin wt-branch \
            > "$TEST_ROOT/output" 2>&1
        [ "$(git -C "$TEST_ROOT/wt" rev-parse HEAD)" = "$caller_head" ]
        [ "$(git -C "$TEST_ROOT/wt" branch --show-current)" = "$caller_branch" ]
        [ "$(git -C "$TEST_ROOT/wt" for-each-ref refs/heads)" = "$caller_refs" ]
        [ "$(git -C "$TEST_ROOT/wt" write-tree)" = "$caller_index" ]
        [ "$(git -C "$TEST_ROOT/wt" status --porcelain)" = "$caller_status" ]
        grep -Fx 'cargo nextest run --workspace --profile ci' "$TEST_ROOT/commands"
        ;;
    push-unreachable-remote)
        setup_repo
        unreachable_sha="1111111111111111111111111111111111111111"
        head="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        rm -f "$TEST_ROOT/commands"
        printf 'refs/heads/main %s refs/heads/main %s\n' "$head" "$unreachable_sha" \
            | "$TEST_ROOT/repo/.githooks/pre-push" origin > "$TEST_ROOT/output" 2>&1
        grep -Fx 'npm run check' "$TEST_ROOT/commands"
        grep -Fx 'cargo check' "$TEST_ROOT/commands"
        ;;
    ui-push)
        setup_repo
        base="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        printf '\n// changed\n' >> "$TEST_ROOT/repo/ui/src/seeded.ts"
        git -C "$TEST_ROOT/repo" add -A
        git -C "$TEST_ROOT/repo" commit -q -m "feat(ui): change"
        head="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        rm -f "$TEST_ROOT/commands"
        printf 'refs/heads/main %s refs/heads/main %s\n' "$head" "$base" \
            | "$TEST_ROOT/repo/.githooks/pre-push" origin > "$TEST_ROOT/output" 2>&1
        grep -Fx 'npm run check' "$TEST_ROOT/commands"
        no_command_matching cargo
        ;;
    tauri-push)
        setup_repo
        base="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        printf '\n// changed\n' >> "$TEST_ROOT/repo/ui/src-tauri/seeded.rs"
        git -C "$TEST_ROOT/repo" add -A
        git -C "$TEST_ROOT/repo" commit -q -m "feat(ui): change Tauri"
        head="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        rm -f "$TEST_ROOT/commands"
        printf 'refs/heads/main %s refs/heads/main %s\n' "$head" "$base" \
            | "$TEST_ROOT/repo/.githooks/pre-push" origin > "$TEST_ROOT/output" 2>&1
        grep -Fx 'cargo machete .' "$TEST_ROOT/commands"
        grep -Fx 'cargo fmt --all -- --check' "$TEST_ROOT/commands"
        grep -Fx 'cargo check' "$TEST_ROOT/commands"
        ;;
    unicode-push)
        setup_repo
        base="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        printf 'export const seeded = 2\n' > "$TEST_ROOT/repo/ui/src/café.ts"
        git -C "$TEST_ROOT/repo" add -A
        git -C "$TEST_ROOT/repo" commit -q -m "feat(ui): add a unicode file"
        head="$(git -C "$TEST_ROOT/repo" rev-parse HEAD)"
        rm -f "$TEST_ROOT/commands"
        printf 'refs/heads/main %s refs/heads/main %s\n' "$head" "$base" \
            | "$TEST_ROOT/repo/.githooks/pre-push" origin > "$TEST_ROOT/output" 2>&1
        grep -Fx 'npm run check' "$TEST_ROOT/commands"
        no_command_matching cargo
        ;;
    all)
        setup_repo
        rm -f "$TEST_ROOT/commands"
        "$TEST_ROOT/repo/.githooks/pre-push" --all > "$TEST_ROOT/output" 2>&1
        grep -Fx 'cargo nextest run --workspace --profile ci' "$TEST_ROOT/commands"
        grep -Fx 'npm run check' "$TEST_ROOT/commands"
        grep -Fx 'cargo check' "$TEST_ROOT/commands"
        ;;
    fail)
        setup_repo
        export TEST_FAIL='cargo fmt --all -- --check'
        run_pre_commit_on crates/ariadne-core/src/lib.rs
        unset TEST_FAIL
        [ "$pre_commit_status" != 0 ]
        grep -Fx 'cargo fmt --all -- --check' "$TEST_ROOT/commands"
        grep -F 'FAILED: cargo fmt --all -- --check' "$TEST_ROOT/output"
        ;;
    *) exit 2 ;;
esac
printf 'PASS: %s\n' "$test_case"
