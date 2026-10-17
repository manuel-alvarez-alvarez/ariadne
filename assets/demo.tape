# Terminal demo of the `ariadne` CLI, recorded with https://github.com/charmbracelet/vhs.
#
# Regenerate with assets/record-demo-cli.sh, never by calling vhs on this
# file directly: that script builds the release binary, creates the
# throwaway daemon this tape types against (never the real ~/.ariadne home),
# runs vhs over this file, and removes the daemon and its scratch
# repository when the recording ends or the script is interrupted.
#
#   bash assets/record-demo-cli.sh
#
# Expects, already set up by that script when this tape runs:
#   - target/release/ariadne on PATH
#   - ARIADNE_HOME=/tmp/ariadne-demo-home, a daemon already running on it,
#     with its REST API on 127.0.0.1:18765 (tcp_listen) — the hidden setup
#     below seeds the goal's tasks over it, the same way
#     assets/record-demo-ui.mjs does
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
# The seeder is started here, before the goal exists: a `Hide`/`Show` pair
# only skips capturing frames, it does not erase the terminal's own
# scrollback, so a command typed and run *between* them still shows once
# `Show` resumes. Backgrounding it before anything is on screen, with its
# own output redirected away, is what actually keeps it off the recording.
# It polls for the goal by title (assets/seed-demo-goal.py), so starting it
# this early is safe.
Type "export ARIADNE_REPO_ROOT=$PWD; export PATH=$PWD/target/release:$PATH; export ARIADNE_HOME=/tmp/ariadne-demo-home; unset ARIADNE_SOCKET ARIADNE_ENDPOINT; cd /tmp/ariadne-demo-repo; set +m; (python3 $ARIADNE_REPO_ROOT/assets/seed-demo-goal.py http://127.0.0.1:18765 opencode:opencode/big-pickle >/tmp/ariadne-demo-seed.log 2>&1 &); clear"
Enter
Sleep 500ms
Show

Type "ariadne repo add /tmp/ariadne-demo-repo"
Sleep 500ms
Enter
Sleep 1.5s

Type "ariadne goal create --title 'Add rate limiting to the login endpoint' --description 'Limit the login endpoint to 5 attempts per minute per IP address. Respond 429 Too Many Requests, with a Retry-After header, once a caller goes over the limit.' --repo /tmp/ariadne-demo-repo --model opencode:opencode/big-pickle"
Sleep 500ms
Enter
Sleep 6s

Type "ariadne task ls"
Sleep 500ms
Enter
Sleep 1.5s

Type "ariadne attention"
Sleep 500ms
Enter
Sleep 1.5s

Type "ariadne models ls --agent opencode"
Sleep 500ms
Enter
Sleep 3s
