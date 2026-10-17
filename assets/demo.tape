# Terminal demo of the `ariadne` CLI, recorded with https://github.com/charmbracelet/vhs.
#
# Regenerate:
#
#   vhs assets/demo.tape
#
# Needs, before running:
#
#   - `cargo build --release`, so target/release/ariadne is the CLI it types
#   - a throwaway daemon, never the real ~/.ariadne home, with its REST API
#     on a loopback port — the hidden setup inside the tape seeds the goal's
#     tasks over it, the same way assets/record-demo-ui.mjs does:
#       rm -rf /tmp/ariadne-demo-home /tmp/ariadne-demo-repo
#       mkdir -p /tmp/ariadne-demo-home /tmp/ariadne-demo-repo
#       git -C /tmp/ariadne-demo-repo init -q -b main
#       bash assets/seed-demo-repo.sh /tmp/ariadne-demo-repo
#       printf 'tcp_listen = "127.0.0.1:18765"\n' > /tmp/ariadne-demo-home/config.toml
#       ARIADNE_HOME=/tmp/ariadne-demo-home target/release/ariadne daemon start
#   - stop it after with the pid `daemon start` printed, never by pattern:
#       ARIADNE_HOME=/tmp/ariadne-demo-home target/release/ariadne daemon stop

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
