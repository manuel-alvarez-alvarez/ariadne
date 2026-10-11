#!/usr/bin/env bash
# Shared helpers for the hooks in this directory. Sourced, never executed.

# Pick the area of a repository-relative path by one rule, checked in order:
# docs/, specs/, assets/, bench/, .github/ and *.md are no area wherever they
# are, ui/src-tauri/ is Tauri, other ui/ is UI, and everything else is Rust.
githooks_area_for() {
    case "$1" in
        docs/*|specs/*|assets/*|bench/*|.github/*|*.md) echo none ;;
        ui/src-tauri/*) echo tauri ;;
        ui/*) echo ui ;;
        *) echo rust ;;
    esac
}

# The crate directory name of a path under crates/<crate>/, or empty.
githooks_crate_for() {
    case "$1" in
        crates/*/*)
            local rest
            rest="${1#crates/}"
            echo "${rest%%/*}"
            ;;
        *) echo "" ;;
    esac
}

# Print a command, then run it. Print and exit 1 on the command that failed.
githooks_run() {
    local git_var

    printf '+ %s\n' "$*"
    if ! (
        while IFS= read -r git_var; do
            unset "$git_var"
        done <<EOF
$(git rev-parse --local-env-vars)
EOF
        "$@"
    ); then
        printf 'FAILED: %s\n' "$*" >&2
        exit 1
    fi
}
