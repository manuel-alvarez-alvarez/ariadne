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

## Refresh ACP agents

When an ACP agent is installed or its command changes after the daemon has
started, reprobe the whole registry:

```sh
ariadne agent refresh
```

The listing shows every agent's id, status and command. A rejected agent also
shows why it could not be used. Use `--format json` when a script needs the
unchanged list returned by the daemon.

## List every session

`ariadne session ls` lists Ariadne's own sessions — an orchestrator's, an
author's, a reviewer's — next to outside sessions: conversations an ACP agent
holds that Ariadne never started. Rows come newest activity first.

```sh
ariadne session ls
ariadne session ls --kind outside --agent codex-acp --dir ~/projects/api
ariadne session ls --status running --attention
```

Its columns are `id`, `title`, `status`, `goal`, `task`, `agent`, `age` and
`tokens`; add `directory` with `--columns`. An outside session leaves
`status`, `goal` and `task` empty. `--kind`, `--agent`, `--status`, `--seat`,
`--goal`, `--task` and `--attention` narrow which rows show; `--dir`, `--since`,
`--until` and `--search` narrow by working directory, activity window and
text; `--limit`, `--cursor` and `--refresh` page the table, and `--all`
fetches every page into one instead. Without `--since` or `--until`, the
table holds only the last 7 days of activity. See [Resuming a
session](resuming-sessions.md) for finding and continuing one of these.

## Connect to a session

`ariadne attach` is an interactive console, not a shell inside the agent. It
shows the session's recorded events and sends each line you type as the next
prompt. The agent keeps working after you leave. It takes a session id, a
task id, a goal id, or an outside session's internal id, from the table
above.

```sh
ariadne attach <goal-id>                 # the orchestrator
ariadne attach <task-id>                 # the task's author
ariadne attach <task-id> --seat reviewer # that task's reviewer
ariadne attach <session-id>              # one specific session, live or ended
ariadne attach <internal-id> --agent codex-acp  # an outside session
```

Attaching an outside id resumes the conversation and opens the console on it
in one step. Attaching an ended Ariadne session revives it the same way,
whether its goal completed or its worktree was already removed. `--agent`
breaks the tie on the rare internal id two agents both hold; otherwise the id
alone is enough. [Resuming a session](resuming-sessions.md) says what a
resumed or revived session can and cannot do.

In a terminal the console is an inline pane. The transcript scrolls in
the terminal's own buffer, so it is still there in your scrollback after you
leave. Three things stay pinned under it: a status row, the input box, and a
footer. The pane is as tall as what it holds: between turns it sits right
under the transcript, and it grows with the block the agent is writing — up to
the whole terminal, where it shows the block's last lines — and shrinks back
once the block has moved into the scrollback.

```
 author · claude:opus · running   ⠹ thinking 12s
────────────────────────────────────────────────────────
❯ the input box
────────────────────────────────────────────────────────
 enter send · shift+enter newline · esc cancel · ctrl-c quit    ↑ 12.4k ↓ 3.1k
```

The status row names the seat, the model and the session's status — `running`
while the agent works, `idle` between turns, `exited` once it has gone, moving
as the session does — and a spinner says "thinking" or "running &lt;tool&gt;"
while a turn runs, with how long the turn has been running next to it: `12s`,
or `1m 04s` past a minute. A prompt you type while a turn runs does not change
what the row says. The count starts again with each turn and is not
shown between turns; attaching in the middle of a turn counts from its prompt.

The footer shows the keys you can press now on the left — to send, to answer
a permission question (`up/down or 1-9 choose · enter answer`), or to confirm that
you want to leave after one Ctrl-C — and on the right the tokens the session
has spent, read (`↑`) and written (`↓`), such as `↑ 12.4k ↓ 3.1k`. The counts
are the whole session's, every launch of it included, and they move each time
a turn ends. Nothing is shown there before the session has spent a token.

Neither row is ever cut off. In a narrow terminal each row leaves out whole
items, the least important first: the status row drops the model, then the
seat, the clock, and what the turn is doing, and keeps the session's status to
the last; the footer drops its later key hints, then the tokens, and keeps the
first hint to the last. Resizing the terminal redraws the pane at the new
size, and a shorter terminal still shows the whole of it. A narrower terminal
gets the pane on its top row, and what was on the screen above the pane moves
into the scrollback.

A resize has two limits, because the terminal moves its rows before the
console hears of the resize, and the console does not ask the terminal where
they went:

- In tmux, a taller window pulls rows of the history down onto the screen.
  The pane is drawn again where it was, over some of those rows, so they are
  gone from the scrollback. Attach again to see the whole transcript.
- On a narrower window, the terminal rewraps the pane before it is drawn
  again. Rows of the old pane that the rewrap moved above it can stay in the
  scrollback.

The input box has a dim rule above and below it, without side borders. Its
prompt is `❯ `, and continued rows align under the text:

```
──────────────────────────────────────────────────────
❯ typed text wraps at the pane width
  and continues on the next row
──────────────────────────────────────────────────────
```

An empty box says `Tell the agent what to do` in dim text. The hint is not
part of the prompt. The box grows to four text rows, then scrolls with the
cursor.

Before the transcript, the scrollback gets a short welcome banner naming the
seat, task (or orchestrator goal), model and effort, repository, and session,
and one blank line under it.
On a narrow terminal it uses the same lines without a box; long titles are
shortened to fit.

The pane opens where the cursor is, which the console asks the terminal for
once. A terminal that does not answer — a pseudo-terminal with nothing behind
it, as `script` gives a process with no terminal of its own — keeps the
console waiting a few seconds, and then the same pane opens from the bottom
row of the screen. Nothing else changes: the transcript still scrolls into
your scrollback, and the console never switches to the alternate screen. The
terminal is not asked again after that, however many blocks scroll past and
however often the pane grows or shrinks.

The agent's text streams in as it is written and renders as markdown:
headings, bold, code spans, fenced code under its language, lists and task
lists, quotes, tables, and links with their URL in plain text. A thought is
dimmed and folded to a few lines. A plan is a checklist that counts what is done
(`plan 2/5`): `☐` pending, `◐` in progress, `☑` done and dimmed. A briefing
or a nudge from the daemon shows under `» daemon`, folded to its first six
lines. An error shows whole after `✗`. A note says what happened in words,
such as `turn cancelled`; a turn that simply ends adds nothing. One blank line
separates two blocks.

A tool call is one block. Its head line says what the call did and on what:
a status mark (`○` pending, `◐` running, `✓` done, `✗` failed), a glyph for
the kind of call (`$` a command, `≡` a read, `✎` an edit, `⌫` a delete, `→` a
move, `⌕` a search, `⇣` a fetch, `∴` a thought, `⇄` a mode switch, `◇` any
other kind), then the command, the path and line, the pattern, or the URL.
Once the call has ended, the head says how long it took. A call that a
cancelled turn left running keeps its `◐`, and goes to the scrollback with
the rest of the turn.
Its output is folded to its last lines under the head, with a count of the
lines left out, and hangs from the head by `⎿`. A file change is a diff: the
file's name, then the hunks with added and removed lines in colour, folded
past a page with a count.

```
✓ $ cargo nextest run  8.2s
  ⎿ … 41 more lines
    Summary [   7.910s] 345 tests run: 345 passed, 0 skipped

✓ ✎ src/main.rs
  ⎿ src/main.rs
    @@ -1,3 +1,3 @@
     fn main() {
    -    println!("hello");
    +    println!("hello, world");
     }
```

| Key | What it does |
| --- | --- |
| Enter | Sends what you typed, or answers the permission question on screen |
| Shift+Enter, Alt+Enter, Ctrl-J | Starts a new line without sending. Shift+Enter needs a terminal that reports it through the kitty keyboard protocol; Alt+Enter and Ctrl-J work in every terminal |
| `\` then Enter | Removes the final backslash and starts a new line without sending |
| ↑, ↓ | Moves by a wrapped row; at the first or last row, moves through prompt history |
| Ctrl-A, Ctrl-E | Moves to the start or the end of the line |
| Ctrl-U, Ctrl-K | Deletes to the start or the end of the line |
| Ctrl-W | Deletes the word before the cursor |
| Alt+←, Alt+→ | Moves back or forward one word (Alt-B and Alt-F do the same) |
| ↑, ↓, or 1 to 9 during a question | Chooses a permission option instead of moving through input or history |
| Escape | Cancels the running turn |
| Ctrl-C twice, Ctrl-D | Leaves the console; the session keeps running, and the console says how to attach again |

Pasting puts the text into the input box where the cursor is, line breaks
and all; nothing is sent until you press Enter. Wide characters and emoji
take the two columns they draw on, in the transcript and in the box alike.
Moving down past the newest history entry restores the draft you were typing.
History includes prompts typed in this console and its snapshot, never prompts
sent by the daemon.

A typed prompt shows as `❯ text` straight away, with a coloured bar down its
left edge. Until the daemon takes it, it carries a dim `queued` tag: a prompt
typed while a turn runs waits for that turn to end. A prompt the daemon
refuses loses its tag, and the reason shows under it after `✗`. When the
session ends, the console closes and says how to revive the session with
`ariadne session resume`. If the daemon's stream
drops, the status row says "reconnecting" until it is back, and nothing
already on screen is printed twice.

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
narrow the snapshot, which is the session's 200 newest events rather than
every turn it ever ran; during `--follow`, `--kind` also narrows new events.
`--format json` keeps each daemon event object unchanged for scripts.

Use `ariadne session send <session-id> "Please explain the failure"`
when a script or a one-line response is enough. When a session is waiting on a
permission request, the console shows the call it asks about — its head line,
and the whole command or the diff under it — and then its choices; pick one
with the arrow keys or its number key. See
[Permission modes](permissions.md).

```
─ permission ───────────────────────────────────
Bash
○ $ cargo build
    cargo build
    cargo nextest run
❯ 1. Allow
  2. Reject
────────────────────────────────────────────────
```

Two rules frame the question. Once you answer, the console keeps only the
question, the call's head line and `↳` with the option you chose.

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
shows the available values, and its `rank` column shows the rank each has, or
a dash for a model nobody ranked.

Rank a model so the orchestrator staffs the smallest one a task earns:

```sh
ariadne models rank codex-acp:<model-id> frontier
ariadne models rank codex-acp:<model-id> --clear   # back to unranked
```

The four ranks: `frontier` is the agent's most capable model, `balanced` is
its everyday one, `fast` is its quick and cheap one, and `local` runs on the
user's own machine. A task can change its permission handling before it
starts:

```sh
ariadne task update <task-id> --model codex-acp:<model-id> --effort high
ariadne task create <goal-id> --title "Write the release notes" \
    --author documentation=codex-acp:<model-id> --no-reviewer --landing none \
    --permission-mode ask
```

`task update` applies only while a task is pending or ready. See
[Permission modes](permissions.md) and [Resuming a session](resuming-sessions.md)
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
