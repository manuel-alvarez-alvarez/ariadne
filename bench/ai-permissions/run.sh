#!/usr/bin/env bash
# Run every AI permission evaluator over the same cases and print one table.
#
# `laya` and `kev` load their models in process and cannot share an
# interpreter (Laya needs Python 3.14, Kev 3.12/3.13 and another torch), so
# each gets a virtual environment of its own. This script creates the ones
# that are missing, runs `run.py run` once per evaluator in the right one,
# collects the per-case CSVs in a directory for this run, and prints one table
# over all of them with `run.py report`.
#
# Everything here is bash 3.2 (macOS) compatible.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
AI_HOME="${ARIADNE_AI_PERMISSIONS_HOME:-$HOME/.ariadne/ai-permissions}"

# Neither is on PyPI (the `laya` there is another project): Laya ships a wheel
# with each GitHub release, and Kev installs from its repository.
LAYA_RELEASES="https://api.github.com/repos/NandhaKishorM/laya/releases/latest"
KEV_REPOSITORY="git+https://github.com/jaredpalmer/kev"

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Run the AI permission evaluators, each in its own virtual environment, and
print one table over their results.

Options:
  -e, --evaluator NAME[=CONFIG]  evaluator to run (laya or kev), with an
                                 optional config; repeat for each one.
                                 Default: laya and kev
  -c, --cases PATH               case file or directory; repeat for each one.
                                 Default: run.py's development cases
      --heldout                  run the held-out cases instead
      --real                     also score approved requests from ariadne.db
      --sweep                    print each evaluator's threshold sweep
  -o, --out DIR                  where the CSVs, logs and report go.
                                 Default: out/runs/<UTC time>, linked as out/latest
      --setup                    only create the virtual environments
      --rebuild                  recreate the virtual environments first,
                                 with the latest Laya and Kev
  -h, --help                     show this help

Environment:
  ARIADNE_AI_PERMISSIONS_HOME    venvs and the Hugging Face cache
                                 (default: ~/.ariadne/ai-permissions)
  LAYA_PYTHON, KEV_PYTHON        interpreter to build each venv with
  LAYA_WHEEL                     Laya wheel URL or path to install instead of
                                 the latest release
EOF
}

die() { echo "error: $*" >&2; exit 1; }
say() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

evaluators=()
cases=()
real=0
sweep=0
out=""
setup_only=0
rebuild=0
while [ $# -gt 0 ]; do
    case "$1" in
        -e|--evaluator) [ $# -ge 2 ] || die "$1 needs a value"; evaluators+=("$2"); shift 2 ;;
        -c|--cases) [ $# -ge 2 ] || die "$1 needs a value"; cases+=("$2"); shift 2 ;;
        --heldout) cases+=("$HERE/cases/safe-heldout.jsonl" "$HERE/cases/adversarial-heldout.jsonl"); shift ;;
        --real) real=1; shift ;;
        --sweep) sweep=1; shift ;;
        -o|--out) [ $# -ge 2 ] || die "$1 needs a value"; out="$2"; shift 2 ;;
        --setup) setup_only=1; shift ;;
        --rebuild) rebuild=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; die "unknown option $1" ;;
    esac
done
[ ${#evaluators[@]} -gt 0 ] || evaluators=(laya kev)

export HF_HOME="${HF_HOME:-$AI_HOME/hf}"

# The backend is the evaluator name before any `=config`.
backend_of() { echo "${1%%=*}"; }

first_python() {
    local candidate
    for candidate in "$@"; do
        if command -v "$candidate" >/dev/null 2>&1; then command -v "$candidate"; return 0; fi
    done
    return 1
}

# The wheel of Laya's latest GitHub release.
latest_laya_wheel() {
    curl -fsSL "$LAYA_RELEASES" | "$(first_python python3)" -c '
import json, sys
wheels = [a["browser_download_url"] for a in json.load(sys.stdin)["assets"] if a["name"].endswith(".whl")]
if not wheels:
    sys.exit("the latest Laya release has no wheel")
print(wheels[0])'
}

# Create (or reuse) the venv for one backend and print its interpreter. A venv
# that already imports its package is used as it is; a missing one, or any
# after --rebuild, is built with the latest Laya or Kev.
ensure_venv() {
    local backend="$1" venv module python spec
    case "$backend" in
        laya)
            venv="$AI_HOME/laya-venv"; module=laya
            python="${LAYA_PYTHON:-$(first_python python3.14 || true)}"
            ;;
        kev)
            venv="$AI_HOME/kev-venv"; module=kev
            python="${KEV_PYTHON:-$(first_python python3.13 python3.12 || true)}"
            ;;
        *) die "no virtual environment for $backend" ;;
    esac
    if [ "$rebuild" -eq 1 ] && [ -d "$venv" ]; then
        echo "removing $venv" >&2
        rm -rf "$venv"
    fi
    if [ -x "$venv/bin/python" ] && "$venv/bin/python" -c "import $module" 2>/dev/null; then
        echo "$venv/bin/python"
        return 0
    fi
    [ -n "$python" ] || die "no interpreter for $backend: install Python (laya 3.14, kev 3.13) or set $(echo "$backend" | tr "[:lower:]" "[:upper:]")_PYTHON"
    case "$backend" in
        laya)
            if [ -n "${LAYA_WHEEL:-}" ]; then
                spec="laya[serve] @ $LAYA_WHEEL"
            else
                spec="laya[serve] @ $(latest_laya_wheel)" || die "could not find the latest Laya release"
            fi
            ;;
        kev) spec="kev[serve] @ $KEV_REPOSITORY" ;;
    esac
    echo "installing $spec into $venv with $python" >&2
    [ -x "$venv/bin/python" ] || "$python" -m venv "$venv" >&2
    "$venv/bin/python" -m pip install --quiet --upgrade pip >&2
    "$venv/bin/python" -m pip install --quiet --upgrade "$spec" >&2
    echo "$venv/bin/python"
}

# The interpreter each evaluator runs under.
python_for() {
    case "$(backend_of "$1")" in
        laya|kev) ensure_venv "$(backend_of "$1")" ;;
        *) die "unknown evaluator $(backend_of "$1") (choose laya or kev)" ;;
    esac
}

say "Preparing environments"
pythons=()
for evaluator in "${evaluators[@]}"; do
    python="$(python_for "$evaluator")"
    pythons+=("$python")
    echo "$(backend_of "$evaluator"): $python"
done
[ "$setup_only" -eq 0 ] || exit 0

if [ -z "$out" ]; then
    out="$HERE/out/runs/$(date -u +%Y%m%dT%H%M%SZ)"
    link_latest=1
else
    link_latest=0
fi
mkdir -p "$out/logs"

run_args=(run --out "$out")
[ ${#cases[@]} -eq 0 ] || run_args+=(--cases "${cases[@]}")
[ "$real" -eq 0 ] || run_args+=(--real)

failed=()
for i in "${!evaluators[@]}"; do
    evaluator="${evaluators[$i]}"
    say "Running $evaluator"
    log="$out/logs/$(backend_of "$evaluator").log"
    if ! "${pythons[$i]}" "$HERE/run.py" "${run_args[@]}" --evaluator "$evaluator" 2>&1 | tee "$log"; then
        failed+=("$evaluator")
        echo "$evaluator failed; its output is in $log" >&2
    fi
done

if [ "$link_latest" -eq 1 ]; then
    ln -sfn "$out" "$HERE/out/latest"
fi

if ls "$out"/*.csv >/dev/null 2>&1; then
    say "Results ($out)"
    report_args=(report "$out")
    [ "$sweep" -eq 0 ] || report_args+=(--sweep)
    "$(first_python python3)" "$HERE/run.py" "${report_args[@]}" | tee "$out/report.txt"
fi

if [ ${#failed[@]} -gt 0 ]; then
    echo >&2
    die "failed: ${failed[*]}"
fi
