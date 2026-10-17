# Terminal demo of the `ariadne` CLI, recorded with https://github.com/charmbracelet/vhs.
#
# Regenerate with assets/record-demo-cli.sh, never by calling vhs on this
# file directly: that script builds the release binary, creates the
# throwaway daemon this tape types against (never the real ~/.ariadne home),
# registers assets/demo-orchestrator-agent.py as the orchestrator's agent,
# runs vhs over this file, and removes the daemon and its scratch
# repository when the recording ends or the script is interrupted.
#
#   bash assets/record-demo-cli.sh
#
# Expects, already set up by that script when this tape runs:
#   - target/release/ariadne on PATH
#   - ARIADNE_HOME=/tmp/ariadne-demo-home, a daemon already running on it,
#     with its REST API on 127.0.0.1:18765 (tcp_listen), and
#     assets/demo-orchestrator-agent.py registered as the acp agent
#     `demo-orchestrator` — a hidden step after the console exchange below
#     runs to completion before `task ls` asks, seeding the goal's tasks
#     over that REST API the same way assets/record-demo-ui.mjs does, only
#     once the stub has accepted the typed answer: seed-demo-goal.py adds
#     the tasks and finalizes the plan as soon as it finds the goal, so
#     starting it any earlier, or not waiting it out, would let the tasks
#     and the finalized plan show up before the answer did, or not show up
#     at all yet when `task ls` runs
#   - /tmp/ariadne-demo-repo, a git repository seeded with
#     assets/seed-demo-repo.sh

Output assets/demo.gif

Require ariadne

Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 20
Set Width 2000
Set Height 850
Set Padding 24
Set WindowBar Rings
Set BorderRadius 10
Set Theme "Dracula"
Set TypingSpeed 45ms

Hide
# A `Hide`/`Show` pair only skips capturing frames, it does not erase the
# terminal's own scrollback, so a command typed and run *between* them would
# still show once `Show` resumes — the `clear` at the end of each hidden
# block is what actually keeps it off the recording. This first one only
# sets up the shell: the task seeder itself is started in a later hidden
# block, after the console exchange below, never here (see the note at the
# top of this file).
Type "export ARIADNE_REPO_ROOT=$PWD; export PATH=$PWD/target/release:$PATH; export ARIADNE_HOME=/tmp/ariadne-demo-home; unset ARIADNE_SOCKET ARIADNE_ENDPOINT; cd /tmp/ariadne-demo-repo; set +m; clear"
Enter
Sleep 500ms
Show

Type "ariadne repo add /tmp/ariadne-demo-repo"
Sleep 500ms
Enter
Sleep 1.5s

Type "GOAL_ID=$(ariadne goal create -q --title 'Add rate limiting to the login endpoint' --description 'Limit the login endpoint to 5 attempts per minute per IP address. Respond 429 Too Many Requests, with a Retry-After header, once a caller goes over the limit.' --repo /tmp/ariadne-demo-repo --model demo-orchestrator:demo-model)"
Sleep 500ms
Enter
Sleep 1.5s

Type "ariadne goal attach $GOAL_ID"
Sleep 500ms
Enter
Sleep 4s

Type "Per IP address."
Sleep 500ms
Enter
Sleep 3.5s

Ctrl+C
Sleep 300ms
Ctrl+C
Sleep 500ms

Hide
# Only now, after the console showed the stub accept the typed answer, is
# the task seeder run: assets/seed-demo-goal.py adds the goal's two tasks
# and finalizes the plan as soon as it finds the goal, which already exists
# by this point, so it would race ahead of the answer if it ran any
# earlier. Run in the foreground, never backgrounded, so the shell does not
# return — and `clear` does not run — until it is done; its own waits are
# each bounded (the goal and the orchestrator session it looks for already
# exist, so those resolve at once, and its last, best-effort wait for the
# first task to leave pending is capped at 10s), so the `Sleep` below,
# itself hidden and so free, only has to outlast that bound once, not guess
# at a completion time the way a backgrounded command's fixed pause would.
Type "python3 $ARIADNE_REPO_ROOT/assets/seed-demo-goal.py http://127.0.0.1:18765 opencode:opencode/big-pickle >/tmp/ariadne-demo-seed.log 2>&1; clear"
Enter
Sleep 15s
Show

Type "ariadne task ls"
Sleep 500ms
Enter
Sleep 1.5s

Type "ariadne attention"
Sleep 500ms
Enter
Sleep 1.5s
