#!/usr/bin/env bash
# Run as: bash scripts/tests/check-migrations.sh CASE, for one of the cases
# below. Builds a throwaway repository with one shipped migration on `main`,
# then drives scripts/check-migrations.sh against a branch built for the case.
set -Eeuo pipefail
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
test_case="${1:?name a test case}"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

git -C "$TEST_ROOT" init -q -b main
git -C "$TEST_ROOT" config user.email test@example.com
git -C "$TEST_ROOT" config user.name "Test"
mkdir -p "$TEST_ROOT/crates/ariadne-store/migrations" "$TEST_ROOT/scripts"
cp "$REPO_DIR/scripts/check-migrations.sh" "$TEST_ROOT/scripts/"
chmod 755 "$TEST_ROOT/scripts/check-migrations.sh"
printf 'CREATE TABLE a (id INTEGER);\n' \
    > "$TEST_ROOT/crates/ariadne-store/migrations/0001_init.sql"
git -C "$TEST_ROOT" add -A
git -C "$TEST_ROOT" commit -q -m "chore: seed repository"

run_guard() {
    if "$TEST_ROOT/scripts/check-migrations.sh" > "$TEST_ROOT/output" 2>&1; then
        echo 0
    else
        echo "$?"
    fi
}

case "$test_case" in
    reject-edit)
        git -C "$TEST_ROOT" checkout -q -b change
        printf -- '-- edited\n' >> "$TEST_ROOT/crates/ariadne-store/migrations/0001_init.sql"
        git -C "$TEST_ROOT" commit -q -am "test: edit the shipped migration"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0001_init.sql: a migration already on main must not be modified or deleted" \
            "$TEST_ROOT/output"
        ;;
    reject-delete)
        git -C "$TEST_ROOT" checkout -q -b change
        git -C "$TEST_ROOT" rm -q crates/ariadne-store/migrations/0001_init.sql
        git -C "$TEST_ROOT" commit -q -m "test: delete the shipped migration"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0001_init.sql: a migration already on main must not be modified or deleted" \
            "$TEST_ROOT/output"
        ;;
    accept-add)
        git -C "$TEST_ROOT" checkout -q -b change
        printf 'ALTER TABLE a ADD COLUMN b TEXT;\n' \
            > "$TEST_ROOT/crates/ariadne-store/migrations/0002_add_b.sql"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: add a new migration"
        [ "$(run_guard)" = 0 ]
        ;;
    reject-low-number)
        git -C "$TEST_ROOT" checkout -q -b change
        printf 'ALTER TABLE a ADD COLUMN c TEXT;\n' \
            > "$TEST_ROOT/crates/ariadne-store/migrations/0001_other.sql"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: add a migration numbered no higher than 0001"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0001_other.sql: version 1 is not greater than the highest shipped migration, 1" \
            "$TEST_ROOT/output"
        ;;
    reject-bad-name)
        git -C "$TEST_ROOT" checkout -q -b change
        printf 'ALTER TABLE a ADD COLUMN d TEXT;\n' \
            > "$TEST_ROOT/crates/ariadne-store/migrations/add_d.sql"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: add a migration with no version prefix"
        [ "$(run_guard)" = 1 ]
        grep -Fq "add_d.sql: name a new migration NNNN_description.sql" "$TEST_ROOT/output"
        ;;
    unrelated-change)
        git -C "$TEST_ROOT" checkout -q -b change
        printf 'fn seeded() {}\n' > "$TEST_ROOT/unrelated.rs"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: touch a file outside migrations"
        [ "$(run_guard)" = 0 ]
        ;;
    *)
        echo "unknown case: $test_case" >&2
        exit 1
        ;;
esac

printf 'PASS: check-migrations %s\n' "$test_case"
