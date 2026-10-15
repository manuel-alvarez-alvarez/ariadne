#!/usr/bin/env bash
# Guards crates/ariadne-store/migrations/: a migration already on `main` ships
# with a checksum sqlx recorded in every database that has run it, so editing
# or deleting the file makes that database refuse to open (016, rule 9).
#
# Diffs the migrations directory between HEAD and the latest commit of
# `main` it is reachable from. A file already on that commit must still be
# present and byte-for-byte unchanged. A new file must be named
# NNNN_description.sql, with a version number greater than every migration
# already there (and than every other new one in the same diff).
#
# Run by hand: scripts/check-migrations
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MIGRATIONS_DIR="crates/ariadne-store/migrations"

base_ref="main"
if git rev-parse --verify -q origin/main >/dev/null; then
    base_ref="origin/main"
fi
base_sha="$(git merge-base "$base_ref" HEAD)"

status=0

# The highest version number already shipped at $base_sha.
protected_max=0
while IFS= read -r path; do
    [ -z "$path" ] && continue
    name="$(basename "$path")"
    case "$name" in
        [0-9][0-9][0-9][0-9]_*.sql)
            version=$((10#${name%%_*}))
            [ "$version" -gt "$protected_max" ] && protected_max=$version
            ;;
    esac
done < <(git ls-tree -r --name-only "$base_sha" -- "$MIGRATIONS_DIR")

new_versions=" "

while IFS=$'\t' read -r change path; do
    [ -z "$change" ] && continue
    name="$(basename "$path")"
    case "$change" in
        A)
            case "$name" in
                [0-9][0-9][0-9][0-9]_*.sql)
                    version=$((10#${name%%_*}))
                    if [ "$version" -le "$protected_max" ]; then
                        echo "check-migrations: $path: version $version is not greater than the highest shipped migration, $protected_max" >&2
                        status=1
                    elif [ "${new_versions#* "$version" }" != "$new_versions" ]; then
                        echo "check-migrations: $path: version $version repeats another new migration in this change" >&2
                        status=1
                    else
                        new_versions="$new_versions$version "
                    fi
                    ;;
                *)
                    echo "check-migrations: $path: name a new migration NNNN_description.sql" >&2
                    status=1
                    ;;
            esac
            ;;
        M | D)
            echo "check-migrations: $path: a migration already on $base_ref must not be modified or deleted" >&2
            status=1
            ;;
    esac
done < <(git diff --no-renames --name-status "$base_sha" HEAD -- "$MIGRATIONS_DIR")

if [ "$status" = 0 ]; then
    echo "check-migrations: every migration already on $base_ref is unchanged, every new one outranks it"
fi

exit "$status"
