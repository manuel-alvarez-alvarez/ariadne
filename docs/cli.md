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
names the seat, the model and the session's status, and a spinner says
"thinking" or "running &lt;tool&gt;" while a turn runs.

The agent's text streams in as it is written and renders as markdown:
headings, bold, code spans, fenced code and lists. A thought is dimmed and
folded to a few lines. A tool call is one line with its status, its name and
its input, with its output folded under it and a file change coloured as a
diff. A plan is a checklist.

| Key | What it does |
| --- | --- |
| Enter | Sends what you typed, or answers the permission question on screen |
| Shift+Enter, Alt+Enter | Starts a new line in the input box |
| ↑ ↓, or 1 to 9 | Chooses an option of a permission question |
| Escape | Cancels the running turn |
| Ctrl-C twice, Ctrl-D | Leaves the console; the session keeps running |

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
permission request, the console offers its choices; pick one with the arrow
keys or its number key. See [Permission modes](permissions.md).

The desktop app has the same console in a session's detail view, drawn as a
terminal pane. The agent's text streams in as it is written and renders as
markdown. A thought is dimmed and folded; open it with its toggle. A tool call
is one row with its status, its name and its input; open it to read the
output, and a file change opens to a diff. A plan is a checklist that updates
in place. Type in the input line at the bottom: Enter sends, Shift+Enter
starts a new line, and the line shows as `> text` until the daemon confirms it.
While a turn runs, the Stop button beside the input, or Escape in the input,
cancels it; the transcript then reads "Stopped". A permission request lists
its options inline; select one, or, with the input empty, press its number
key (1 to 9). The pane follows new output until you scroll up, and "Jump to
latest" takes you back. The agent receives the same input whichever console
you use.

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
