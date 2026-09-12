# Using the CLI

Use `ariadne` to create goals, follow the work, and connect to an agent's
console. `ariadne --help` is the complete command reference.

## A first run

Choose a model that `ariadne models ls` reports, then create a goal with its
full `<agent-id>:<model-id>` name.

```sh
ariadne daemon start
ariadne doctor
ariadne models ls

ariadne repo add ~/projects/api --description "the public API"
ariadne goal create --title "Add rate limiting" --repo ~/projects/api \
    --model codex-acp:<model-id>
ariadne goal attach <goal-id>
```

The final command opens the orchestrator console. Answer its questions there;
it writes the plan and starts the agreed tasks.

## Connect to a session

`ariadne attach` is an interactive console, not a shell inside the agent. It
shows the session's recorded events and sends each line you type as the next
prompt. The agent keeps working after you leave.

```sh
ariadne attach <goal-id>                 # the orchestrator
ariadne attach <task-id>                 # the task's author
ariadne attach <task-id> --seat reviewer # that task's reviewer
ariadne attach <session-id>              # one specific session
```

In a terminal the console is a small inline pane. The transcript scrolls in
the terminal's own buffer, so it is still there in your scrollback after you
leave; a status line and the input box stay pinned under it. The status line
names the seat, the model and the session's status — `running` while the
agent works, `idle` between turns, `exited` once it has gone, moving as the
session does — and a spinner says
"thinking" or "running &lt;tool&gt;" while a turn runs, with how long the
turn has been running next to it: `12s`, or `1m 04s` past a minute. The
count starts again with each turn and is not shown between turns; attaching
in the middle of a turn counts from its prompt. Resizing the terminal
redraws the pane at the new size.

The pane opens where the cursor is, which the console asks the terminal for
once. A terminal that does not answer — a pseudo-terminal with nothing behind
it, as `script` gives a process with no terminal of its own — keeps the
console waiting a few seconds, and then the same pane opens from the bottom
row of the screen. Nothing else changes: the transcript still scrolls into
your scrollback, and the console never switches to the alternate screen. The
terminal is not asked again after that, however many blocks scroll past.

The agent's text streams in as it is written and renders as markdown:
headings, bold, code spans, fenced code and lists. A thought is dimmed and
folded to a few lines. A plan is a checklist.

A tool call is one block. Its head line says what the call did and on what:
a status mark (`○` pending, `●` running, `✓` done, `✗` failed), a glyph for
the kind of call (`$` a command, `≡` a read, `✎` an edit, `⌫` a delete, `→` a
move, `⌕` a search, `↓` a fetch), then the command, the path and line, the
pattern, or the URL. Once the call has ended, the head says how long it took.
Its output is folded to its last lines under the head, with a count of the
lines left out. A file change is a diff: the file's name, then the hunks with
added and removed lines in colour, folded past a page with a count.

```
✓ $ cargo nextest run  8.2s
    … 41 more lines
    Summary [   7.910s] 345 tests run: 345 passed, 0 skipped
✓ ✎ src/main.rs
    src/main.rs
    @@ -1,3 +1,3 @@
     fn main() {
    -    println!("hello");
    +    println!("hello, world");
     }
```

| Key | What it does |
| --- | --- |
| Enter | Sends what you typed, or answers the permission question on screen |
| Shift+Enter, Alt+Enter | Starts a new line in the input box |
| Ctrl-A, Ctrl-E | Moves to the start or the end of the line |
| Ctrl-U, Ctrl-K | Deletes to the start or the end of the line |
| Ctrl-W | Deletes the word before the cursor |
| Alt+←, Alt+→ | Moves back or forward one word (Alt-B and Alt-F do the same) |
| ↑ ↓, or 1 to 9 | Chooses an option of a permission question |
| Escape | Cancels the running turn |
| Ctrl-C twice, Ctrl-D | Leaves the console; the session keeps running |

Pasting puts the text into the input box where the cursor is, line breaks
and all; nothing is sent until you press Enter. Wide characters and emoji
take the two columns they draw on, in the transcript and in the box alike.

A typed prompt shows as `> text` straight away, and is replaced when the
daemon confirms it. If the daemon's stream drops, the status line says
"reconnecting" until it is back, and nothing already on screen is printed
twice.

With stdin or stdout redirected there is no pane to draw: `ariadne attach`
then prints one `kind · summary` line per event and numbered options for a
permission question, and reads one prompt per line, which is what a script
wants.

Use `ariadne session logs <session-id> --follow` to read without sending
input. It prints complete prompts and replies, dimmed thoughts, plan status,
tool input and output, diffs, and permission answers as timestamped blocks.
The task form finds the session for you:

```sh
ariadne session logs <session-id> --tail 20
ariadne session logs <session-id> --since 10m --kind agent_message
ariadne task logs <task-id> --seat reviewer --follow
```

`--tail N` keeps the last N blocks. `--since` accepts RFC 3339 or a duration
such as `10m`. Repeat `--kind` to include several event kinds. These filters
narrow the snapshot; during `--follow`, `--kind` also narrows new events.
`--format json` keeps each daemon event object unchanged for scripts.

Use `ariadne session send <session-id> "Please explain the failure"`
when a script or a one-line response is enough. When a session is waiting on a
permission request, the console shows the call it asks about — its head line,
and the whole command or the diff under it — and then its choices; pick one
with the arrow keys or its number key. See
[Permission modes](permissions.md).

```
? Bash
  $ cargo build
    cargo build
    cargo nextest run
  › 1. Allow
    2. Reject
```

The desktop app shows the same console in a session's detail view, in a
terminal emulator. The daemon draws the pane above into it, so everything on
this page works there the same way: the input box and its keys, Escape to
cancel a turn, the picker for a permission request. Ctrl-C twice or Ctrl-D
closes that console and leaves the session running; the Reopen button opens
it again. ⌘V on macOS, or Ctrl+Shift+V, pastes into the input box, and ⌘C or
Ctrl+Shift+C copies what is selected in the pane. The pane takes the app's
colours and font in both themes, redraws at the new size when its panel is
resized, and keeps the transcript in its scrollback. If the daemon goes away
mid-session the pane says it is reconnecting, and draws the console afresh
once it is back. The agent receives the same input whichever console you use.

## Work with tasks

```sh
ariadne attention                         # sessions and tasks needing you
ariadne task ls --goal <goal-id>          # current tasks
ariadne session ls --attention            # only sessions waiting for input
ariadne task inspect <task-id>            # task, agents, diff, and history
ariadne task messages <task-id> --full    # agent conversation about the task
ariadne task diff <task-id>               # proposed change
```

Models and effort are always supplied by ACP discovery. `ariadne models ls`
shows the available values. A task can change its permission handling before
it starts:

```sh
ariadne task update <task-id> --model codex-acp:<model-id> --effort high
ariadne task create <goal-id> --title "Cut a release" \
    --author release=codex-acp:<model-id> --no-reviewer --landing none \
    --permission-mode ask
```

`task update` applies only while a task is pending or ready. See
[Permission modes](permissions.md) and [Adopting a session](adopting-sessions.md)
for the related session workflows.

## Output and troubleshooting

Listings print tables by default. Add `--format json` for JSON, `-q` for ids
only, `-o wide` for every column, or `--columns id,title,age` for selected
columns. `--no-pager` writes long output directly to standard output.

```sh
ariadne task ls -o wide
ariadne task ls --columns id,title,age
ariadne task ls -q | xargs -n1 ariadne task inspect
ariadne doctor
```

`ariadne doctor` checks the CLI, daemon, configuration, service, and ACP
registry. It distinguishes an agent command your shell cannot find from one
the daemon cannot find or cannot use.
