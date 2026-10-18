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
mkdir -p "$TEST_ROOT/crates/ariadne-store/migrations" \
    "$TEST_ROOT/crates/ariadne-store/telemetry-migrations" "$TEST_ROOT/scripts"
cp "$REPO_DIR/scripts/check-migrations.sh" "$TEST_ROOT/scripts/"
chmod 755 "$TEST_ROOT/scripts/check-migrations.sh"
printf 'CREATE TABLE a (id INTEGER);\n' \
    > "$TEST_ROOT/crates/ariadne-store/migrations/0001_init.sql"
printf 'CREATE TABLE log_records (id INTEGER);\n' \
    > "$TEST_ROOT/crates/ariadne-store/telemetry-migrations/0001_init.sql"
git -C "$TEST_ROOT" add -A
git -C "$TEST_ROOT" commit -q -m "chore: seed repository"

run_guard() {
    if "$TEST_ROOT/scripts/check-migrations.sh" > "$TEST_ROOT/output" 2>&1; then
        echo 0
    else
        echo "$?"
    fi
}

# A git blob hash is a pure function of its content, never of which
# repository holds it: writing this real repository's own pre- and
# post-edit `0001_init.sql` bytes into the throwaway one, verbatim,
# reproduces the exact same two blobs `check-migrations.sh` pins, so its
# real, fixed, hardcoded values are what these two cases exercise — no
# override of any kind, in the script or the environment.
real_blob() {
    git -C "$REPO_DIR" cat-file blob "$1" \
        > "$TEST_ROOT/crates/ariadne-store/migrations/0001_init.sql"
}
REAL_EDIT_FROM=f3cd49d06d678e397bd9f5dab62604bf20b8995c
REAL_EDIT_TO=4c9801a083e435c516fe611df0dcfba5faa889ba

# `main` has moved on since a branch forked from it: set up an `origin`
# remote whose `main` has already landed a migration (through a squash
# commit the branch never descends from), and leave the caller on a new
# branch forked from the branch's own, pre-squash commit.
diverge_after_squash() {
    git -C "$TEST_ROOT" remote add origin "$TEST_ROOT/.git"
    git -C "$TEST_ROOT" update-ref refs/remotes/origin/main refs/heads/main
    git -C "$TEST_ROOT" checkout -q -b pre-squash
    printf 'ALTER TABLE a ADD COLUMN b TEXT;\n' \
        > "$TEST_ROOT/crates/ariadne-store/migrations/$1"
    git -C "$TEST_ROOT" add -A
    git -C "$TEST_ROOT" commit -q -m "test: branch adds $1"
    git -C "$TEST_ROOT" checkout -q main
    printf 'ALTER TABLE a ADD COLUMN b TEXT;\n' \
        > "$TEST_ROOT/crates/ariadne-store/migrations/$2"
    git -C "$TEST_ROOT" add -A
    git -C "$TEST_ROOT" commit -q -m "test: main lands $2 through a squash"
    git -C "$TEST_ROOT" update-ref refs/remotes/origin/main refs/heads/main
    git -C "$TEST_ROOT" checkout -q pre-squash
}

case "$test_case" in
    reject-edit)
        git -C "$TEST_ROOT" checkout -q -b change
        printf -- '-- edited\n' >> "$TEST_ROOT/crates/ariadne-store/migrations/0001_init.sql"
        git -C "$TEST_ROOT" commit -q -am "test: edit the shipped migration"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0001_init.sql: a migration already on main must not be modified" \
            "$TEST_ROOT/output"
        ;;
    accept-approved-edit)
        # `main` ships the real pre-edit file, blob for blob, and the
        # branch makes the real post-edit one, blob for blob: the guard's
        # own hardcoded pin names exactly this pair, with no override.
        real_blob "$REAL_EDIT_FROM"
        git -C "$TEST_ROOT" commit -q -am "test: main ships the real pre-edit 0001_init.sql"
        git -C "$TEST_ROOT" checkout -q -b change
        real_blob "$REAL_EDIT_TO"
        git -C "$TEST_ROOT" commit -q -am "test: make exactly the one approved edit"
        [ "$(run_guard)" = 0 ]
        ;;
    reject-near-miss-edit)
        # The same real pre-edit file on `main`, but the branch's edit is
        # the approved one with an extra line, so its blob is neither
        # pinned blob: a different edit of the same migration still fails,
        # proving the pin accepts only its one exact pair.
        real_blob "$REAL_EDIT_FROM"
        git -C "$TEST_ROOT" commit -q -am "test: main ships the real pre-edit 0001_init.sql"
        git -C "$TEST_ROOT" checkout -q -b change
        real_blob "$REAL_EDIT_TO"
        printf -- '-- a further, unapproved edit\n' \
            >> "$TEST_ROOT/crates/ariadne-store/migrations/0001_init.sql"
        git -C "$TEST_ROOT" commit -q -am "test: make a different edit than the one pinned"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0001_init.sql: a migration already on main must not be modified" \
            "$TEST_ROOT/output"
        ;;
    reject-delete)
        git -C "$TEST_ROOT" checkout -q -b change
        git -C "$TEST_ROOT" rm -q crates/ariadne-store/migrations/0001_init.sql
        git -C "$TEST_ROOT" commit -q -m "test: delete the shipped migration"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0001_init.sql: a migration already on main must not be deleted" \
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
    reject-symlink-replace)
        git -C "$TEST_ROOT" checkout -q -b change
        git -C "$TEST_ROOT" rm -q crates/ariadne-store/migrations/0001_init.sql
        mkdir -p "$TEST_ROOT/crates/ariadne-store/migrations"
        ln -s /etc/passwd "$TEST_ROOT/crates/ariadne-store/migrations/0001_init.sql"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: replace the shipped migration with a symlink"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0001_init.sql: a migration already on main must not be modified" \
            "$TEST_ROOT/output"
        ;;
    reject-new-symlink)
        git -C "$TEST_ROOT" checkout -q -b change
        ln -s /etc/passwd "$TEST_ROOT/crates/ariadne-store/migrations/0002_evil.sql"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: add a new migration as a symlink"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0002_evil.sql: a new migration must be a regular file" "$TEST_ROOT/output"
        ;;
    reject-landed-after-squash)
        # main ships 0002_add_b.sql through a squash the branch never
        # descends from; the branch, still holding its own pre-squash
        # commit that added the same path, then edits its own copy.
        diverge_after_squash 0002_add_b.sql 0002_add_b.sql
        printf -- '-- edited on the branch\n' \
            >> "$TEST_ROOT/crates/ariadne-store/migrations/0002_add_b.sql"
        git -C "$TEST_ROOT" commit -q -am "test: edit the branch's own copy after main squashed it in"
        [ "$(run_guard)" = 1 ]
        grep -Fq "0002_add_b.sql: a migration already on origin/main must not be modified" \
            "$TEST_ROOT/output"
        ;;
    reject-version-collision)
        # main ships 0002_theirs.sql through a squash; the branch, forked
        # before that landed, still wants to ship its own 0002.
        diverge_after_squash 0002_mine.sql 0002_theirs.sql
        [ "$(run_guard)" = 1 ]
        grep -Fq "0002_mine.sql: version 2 is not greater than the highest shipped migration, 2" \
            "$TEST_ROOT/output"
        ;;
    unrelated-change)
        git -C "$TEST_ROOT" checkout -q -b change
        printf 'fn seeded() {}\n' > "$TEST_ROOT/unrelated.rs"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: touch a file outside migrations"
        [ "$(run_guard)" = 0 ]
        ;;
    telemetry-reject-edit)
        git -C "$TEST_ROOT" checkout -q -b change
        printf -- '-- edited\n' \
            >> "$TEST_ROOT/crates/ariadne-store/telemetry-migrations/0001_init.sql"
        git -C "$TEST_ROOT" commit -q -am "test: edit the shipped telemetry migration"
        [ "$(run_guard)" = 1 ]
        grep -Fq "telemetry-migrations/0001_init.sql: a migration already on main must not be modified" \
            "$TEST_ROOT/output"
        ;;
    telemetry-accept-add-independent-of-the-other-directory)
        # The other directory ships up to 0002 on main; the telemetry
        # migrations are still numbered from their own 0001, so a new
        # telemetry 0002 is accepted on its own ceiling rather than being
        # compared against the other directory's.
        printf 'ALTER TABLE a ADD COLUMN b TEXT;\n' \
            > "$TEST_ROOT/crates/ariadne-store/migrations/0002_add_b.sql"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: main ships a second migration in the other directory"
        git -C "$TEST_ROOT" checkout -q -b change
        printf 'ALTER TABLE log_records ADD COLUMN service TEXT;\n' \
            > "$TEST_ROOT/crates/ariadne-store/telemetry-migrations/0002_add_service.sql"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: add a new telemetry migration"
        [ "$(run_guard)" = 0 ]
        ;;
    telemetry-reject-low-number)
        git -C "$TEST_ROOT" checkout -q -b change
        printf 'ALTER TABLE log_records ADD COLUMN target TEXT;\n' \
            > "$TEST_ROOT/crates/ariadne-store/telemetry-migrations/0001_other.sql"
        git -C "$TEST_ROOT" add -A
        git -C "$TEST_ROOT" commit -q -m "test: add a telemetry migration numbered no higher than 0001"
        [ "$(run_guard)" = 1 ]
        grep -Fq "telemetry-migrations/0001_other.sql: version 1 is not greater than the highest shipped migration, 1" \
            "$TEST_ROOT/output"
        ;;
    *)
        echo "unknown case: $test_case" >&2
        exit 1
        ;;
esac

printf 'PASS: check-migrations %s\n' "$test_case"
