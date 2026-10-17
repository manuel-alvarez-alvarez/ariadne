#!/usr/bin/env bash
# Record the terminal CLI README demo.
#
# Run this script from the repository root:
#
#   bash assets/record-demo-cli.sh
#
# It builds the release binary, creates an isolated Ariadne home and a
# scratch repository seeded with the same small login HTTP service
# assets/record-demo-ui.mjs seeds (assets/seed-demo-repo.sh), registers
# assets/demo-orchestrator-agent.py as the acp agent `demo-orchestrator`
# (the stub both README recordings run as the orchestrator, so the tape can
# attach to its console and show a real, scripted exchange), starts a
# throwaway daemon with its REST API on a loopback port, and runs
# assets/demo.tape with vhs over it — which attaches to the stub's console,
# types the answer to its question, and only then starts
# assets/seed-demo-goal.py in the background to seed the goal's tasks, the
# same way the UI recording does. The daemon, its home and the scratch
# repository are removed when the recording ends or the script is
# interrupted.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
home=/tmp/ariadne-demo-home
repo=/tmp/ariadne-demo-repo
port=18765
ariadne="$root/target/release/ariadne"

vhs_pid=

# Idempotent, and always the only way out: INT and TERM below just `exit`,
# which runs this same EXIT trap rather than duplicate it.
cleanup() {
  # Each step tolerates its own failure (`|| true`): under `set -e`, a `&&`
  # chain here would abort the rest of this function the moment one command
  # found nothing left to do — vhs already gone, say — skipping the daemon
  # stop and the `rm -rf` that still have to run regardless.
  if [ -n "$vhs_pid" ]; then
    kill "$vhs_pid" >/dev/null 2>&1 || true
    wait "$vhs_pid" 2>/dev/null || true
  fi
  ARIADNE_HOME="$home" "$ariadne" daemon stop >/dev/null 2>&1 || true
  rm -rf "$home" "$repo"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

cd "$root"
cargo build --release

rm -rf "$home" "$repo"
mkdir -p "$home" "$repo"
git -C "$repo" init -q -b main
bash "$root/assets/seed-demo-repo.sh" "$repo"
{
  printf 'tcp_listen = "127.0.0.1:%s"\n' "$port"
  printf '[[acp_agents]]\nid = "demo-orchestrator"\ncommand = ["python3", "%s"]\n' \
    "$root/assets/demo-orchestrator-agent.py"
} > "$home/config.toml"
ARIADNE_HOME="$home" "$ariadne" daemon start

# A plain foreground `vhs ...` would leave INT/TERM pending until vhs itself
# returns: bash only runs a trap between commands, not while one is still
# running in the foreground. Backgrounding it and blocking on `wait
# "$vhs_pid"` instead is what makes a signal here act at once, even while the
# recording is still going — `wait` is interruptible, a foreground exec is
# not. `cleanup`'s own `kill` then covers a signal vhs itself ignored.
PATH="$root/target/release:$PATH" vhs "$root/assets/demo.tape" &
vhs_pid=$!
wait "$vhs_pid"
vhs_pid=
