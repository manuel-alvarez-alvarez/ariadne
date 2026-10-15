#!/usr/bin/env bash
# Guards crates/ariadne-store/migrations/: a migration already on `main` ships
# with a checksum sqlx recorded in every database that has run it, so editing,
# deleting or retyping it (for instance replacing it with a symlink) makes
# that database refuse to open (016, rule 9).
#
# Compares the migrations directory on the latest commit of `main` directly
# against HEAD, never their merge-base: a branch behind main would otherwise
# see a migration main already shipped as absent, and accept the branch's
# own, different copy of it as a fresh addition. A path already on `main`
# must still be present on HEAD at the identical git mode and blob, which
# catches a content edit and a type change (a symlink has a different mode)
# alike. A new path must be a regular file named NNNN_description.sql, with
# a version number greater than every migration `main` already ships (and
# than every other new one in the same change).
#
# Run by hand: scripts/check-migrations.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MIGRATIONS_DIR="crates/ariadne-store/migrations"

base_ref="main"
if git rev-parse --verify -q origin/main >/dev/null; then
    base_ref="origin/main"
fi

status=0
protected_max=0

protected_paths="$(mktemp)"
trap 'rm -f "$protected_paths"' EXIT
git ls-tree -r --name-only "$base_ref" -- "$MIGRATIONS_DIR" > "$protected_paths"

while IFS= read -r path; do
    [ -z "$path" ] && continue
    name="$(basename "$path")"
    case "$name" in
        [0-9][0-9][0-9][0-9]_*.sql)
            version=$((10#${name%%_*}))
            [ "$version" -gt "$protected_max" ] && protected_max=$version
            ;;
    esac

    # The full mode/type/blob entry, not just its content: a symlink swapped
    # in for the same path carries a different mode and so a different entry.
    main_entry="$(git ls-tree "$base_ref" -- "$path" | cut -f1)"
    head_entry="$(git ls-tree HEAD -- "$path" | cut -f1)"
    if [ -z "$head_entry" ]; then
        echo "check-migrations: $path: a migration already on $base_ref must not be deleted" >&2
        status=1
    elif [ "$head_entry" != "$main_entry" ]; then
        echo "check-migrations: $path: a migration already on $base_ref must not be modified" >&2
        status=1
    fi
done < "$protected_paths"

new_versions=" "

while IFS= read -r path; do
    [ -z "$path" ] && continue
    grep -Fxq "$path" "$protected_paths" && continue

    name="$(basename "$path")"
    mode="$(git ls-tree HEAD -- "$path" | cut -d' ' -f1)"
    if [ "$mode" != "100644" ]; then
        echo "check-migrations: $path: a new migration must be a regular file" >&2
        status=1
        continue
    fi

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
done < <(git ls-tree -r --name-only HEAD -- "$MIGRATIONS_DIR")

if [ "$status" = 0 ]; then
    echo "check-migrations: every migration already on $base_ref is unchanged, every new one outranks it"
fi

exit "$status"
