# Terminal demo of the `ariadne` CLI, recorded with https://github.com/charmbracelet/vhs.
#
# Regenerate:
#
#   vhs assets/demo.tape
#
# Needs, before running:
#
#   - `cargo build --release`, so target/release/ariadne is the CLI it types
#   - a throwaway daemon, never the real ~/.ariadne home:
#       rm -rf /tmp/ariadne-demo-home /tmp/ariadne-demo-repo
#       mkdir -p /tmp/ariadne-demo-home /tmp/ariadne-demo-repo
#       git -C /tmp/ariadne-demo-repo init -q -b main
#       git -C /tmp/ariadne-demo-repo commit -q --allow-empty -m "chore: seed the demo repository"
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
Type "export PATH=$PWD/target/release:$PATH; export ARIADNE_HOME=/tmp/ariadne-demo-home; unset ARIADNE_SOCKET ARIADNE_ENDPOINT; cd /tmp/ariadne-demo-repo; clear"
Enter
Sleep 500ms
Show

Type "ariadne repo add /tmp/ariadne-demo-repo"
Sleep 500ms
Enter
Sleep 1.5s

Type "ariadne goal create --title 'Add a health endpoint' --repo /tmp/ariadne-demo-repo --model opencode:opencode/big-pickle"
Sleep 500ms
Enter
Sleep 2s

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
