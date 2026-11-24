# Using the CLI

Use `ariadne` to create goals, follow the work, and connect to an agent's
console. `ariadne --help` is the complete command reference.

## Command telemetry

The CLI sends a command count, duration, and operational logs at `info` and
above to its daemon. Set `RUST_LOG` to select other tracing levels or targets.
Set `ARIADNE_TELEMETRY=off` to disable this for one command or shell session.
The CLI does not send prompt or message text.

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

## Test an AI permission request

Use this command to see how the enabled AI permission model scores one tool
request without granting or recording anything:

```sh
ariadne permissions ai test --tool "git status" --kind execute \
  --input '{"command":"git status","description":"Show the status"}' \
  --option Yes --option "Yes, and don't ask again for git status * commands" --option No \
  --workspace "$PWD"
```

It prints a line such as `ask · allow 62%, deny 5% · danger 21% (allow up to 5%, deny from 65%)`. `--tool` is
the tool call title the model sees: the command for `Bash`, `Edit <path>` for
an edit. Repeat `--option` for each offered option; the first represents an
allowing option. Repeat `--location` for each path the call touches.
`--input` must be valid JSON. `--format json` prints the full response.

## List every session

`ariadne session ls` lists Ariadne's own sessions — an orchestrator's, a
column agent's, a reviewer's, or a keeper's. Rows come newest activity first.

```sh
ariadne session ls
ariadne session ls --agent codex-acp --dir ~/projects/api
ariadne session ls --status running --attention
```

Its columns are `id`, `title`, `status`, `goal`, `task`, `seat`, `agent`,
`model`, `age` and `tokens`; add `directory` with `--columns`. The `seat`
column names a column agent's workflow column beside its seat, `agent ·
develop`; every other seat names only itself. By default, the table holds only the
last 7 days of activity. Use `--since` or `--until` to set an activity bound.
Use `--all` to remove the default 7-day window, include ended sessions, and
fetch every page into one table.
`--agent`, `--status`, `--seat`, `--goal`, `--task` and `--attention`
narrow which rows show; `--dir` and `--search` narrow by working directory and
text; `--limit` and `--cursor` page the table. See [Resuming a
session](resuming-sessions.md) for finding and continuing one of these.

## Connect to a session

`ariadne attach` is an interactive console, not a shell inside the agent. It
shows the session's recorded events and sends each line you type as the next
prompt. The agent keeps working after you leave. It takes a session id, a
task id or a goal id from the table above.

```sh
ariadne attach <goal-id>                 # the orchestrator
ariadne attach <task-id>                 # the agent of the task's current column
ariadne task attach <task-id> --step review   # one column's agent instead
ariadne attach <session-id>              # one specific session, live or ended
```

In a terminal the console is an inline pane as tall as the terminal. Its last
rows are pinned: a status row, the input box, and a footer. So the input box
is always on the bottom rows of the screen, however long or short the
transcript is. The transcript fills the screen above the status row, as in a
chat app: its last line is right over the status row, and the lines before
it go up the screen. Between turns you see the end of the conversation, the
last answer over the status row. During a turn you see the block the agent
is writing over the status row, with your prompt and the blocks before it
above it; when that block is taller than the screen you see its last lines.
Blank rows are on the screen only while the transcript is shorter than the
screen, and they are at the top. A line moves into the terminal's own
scrollback only when it scrolls off the top of the screen, so scroll up to
read the conversation so far. A line of the block the agent is still writing
waits until the block is finished. When you leave, the rest of the
transcript is put on the top rows of the screen, the pane is erased, and
your shell prompt comes back right under the last block, so the transcript
stays in your scrollback, each line once.

```
 agent · develop · claude:opus · running   ⠹ thinking 12s
────────────────────────────────────────────────────────
❯ the input box
────────────────────────────────────────────────────────
 enter send · shift+enter newline · esc cancel · ctrl-c quit    ↑ 12.4k ↓ 3.1k
```

The status row names the seat, the model and the session's status — `running`
while the agent works, `idle` between turns, `exited` once it has gone, moving
as the session does. A column agent's seat is followed by the workflow
column it works, `agent · develop`; every other seat names only itself. A
spinner says "thinking" or "running &lt;tool&gt;"
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

Every fold below is one press of Ctrl-O away. It switches the pane between
folded, which is where each attach starts, and whole, which draws every line
of a tool's output, a diff and a daemon prompt; press it again to fold them
back. A thought is always whole. A reconnect keeps whichever the attach had. A block that
leaves for the scrollback while the pane is whole keeps every line there too,
since a block already in the scrollback cannot be drawn again. A block that
is part-way into the scrollback keeps the fold its first lines had, and
Ctrl-O changes the blocks after it. The footer's
last hint says which way the pane is set, `ctrl-o unfold` or `ctrl-o fold`,
and is the first hint dropped on a narrow terminal. A pending permission
question keeps its own fold, sized to the room its options leave, whichever
way the pane is set.

Neither row is ever cut off. In a narrow terminal each row leaves out whole
items, the least important first: the status row drops the model, then the
seat, the clock, and what the turn is doing, and keeps the session's status to
the last; the footer drops its later key hints, then the tokens, and keeps the
first hint to the last. Resizing the terminal erases the pane and draws it
again at the new size, with the input box on the new bottom rows and the end
of the transcript over it at the new width. On a shorter terminal the lines
that no longer fit move into the scrollback. A taller terminal has blank rows
at the top, since a line in the scrollback does not come back onto the
screen.

A resize has limits in the scrollback, because the terminal moves its rows
before the console hears of the resize, and the console does not ask the
terminal where they went:

- In tmux, a taller window pulls rows of the history down onto the screen.
  The pane erases them when it is drawn again, so they are gone from the
  scrollback. Attach again to see the whole transcript.
- A terminal that moves the top rows of its screen into the scrollback when
  it is made shorter keeps them there, and the pane draws them again, so
  those lines are in the scrollback twice.
- On a narrower window, the terminal rewraps the pane before it is drawn
  again. Rows of the old pane that the rewrap pushed up can stay in the
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

### Agent slash commands

When an agent supplies slash commands, type `/` at the start of the input box
to open its command list. Typing after the `/` filters the names without
regard to case. The list comes from the agent, so it can differ between
agents and sessions. Claude ACP, Codex ACP, and OpenCode supplied a list in
the terminal console check.

| Key | What it does while the list is open |
| --- | --- |
| Up, Down | Move the selected command without moving through prompt history |
| Tab | Put the selected command and a trailing space in the input box |
| Enter | Run a selected command without input; for a command with input, put its name and a trailing space in the box |
| Escape | Close the list and keep the text in the input box |

The hint after a command with input describes its argument and is not sent.
Other editing keys work as they do in the input box. Press Enter after a
command with its input to run it. If the text does not name a listed command,
including a path such as `/tmp/x`, Enter sends it as an ordinary prompt.

The transcript starts with a short welcome banner naming the seat — with its
workflow column too, for a column agent, `agent · develop` — task (or
orchestrator goal), model and effort, repository, and session, and one blank
line under it. It scrolls into the scrollback like any other line.
On a narrow terminal it uses the same lines without a box; long titles are
shortened to fit.

The pane opens where the cursor is, which the console asks the terminal for
once, and then takes the whole screen: what was on the screen above it,
your shell included, moves into the scrollback. A terminal that does not
answer — a pseudo-terminal with nothing behind it, as `script` gives a
process with no terminal of its own — keeps the console waiting a few
seconds, and then the same pane opens from the bottom row of the screen.
Nothing else changes: the transcript still scrolls into your scrollback, and
the console never switches to the alternate screen. The terminal is not
asked again after that, however many blocks scroll past and however often
the terminal is resized.

The agent's text streams in as it is written and renders as markdown:
headings, bold, code spans, fenced code under its language, lists and task
lists, quotes, tables, and links with their URL in plain text. Fenced code
in a language the console knows, such as `rust`, `json` or `sh`, is coloured
in your terminal's own palette: comments dim, strings green, keywords
magenta, numbers cyan and types yellow. Other fenced code is drawn in one
colour, and so is a language the console does not bundle, such as TOML. A
thought is markdown too, dimmed and always drawn whole. A plan
is a checklist that counts what is done (`plan 2/5`): `☐` pending, `◐` in
progress, `☑` done and dimmed. A briefing or a nudge from the daemon shows
under `» daemon`, folded to its first six lines, until Ctrl-O unfolds it. An
error shows whole after `✗`. A note says what happened in words,
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
lines left out, and hangs from the head by `⎿`, until Ctrl-O unfolds it. The
output itself draws in the terminal's own foreground, not dimmed, so the
text you most want to read — a file, a compiler error, a failing test — is
never the hardest to see; the `⎿` gutter, the count of hidden lines, and a
thought stay dim. A file change is a diff: the file's name, then the hunks
with added and removed lines in colour, folded past a page with a count,
until Ctrl-O unfolds it. Where a hunk replaces as many lines as it removes,
the words that changed between a removed line and the one that replaces it
draw bold, so the one word that moved stands out without reading the two
side by side, word by word; a rewritten line that shares few words with its
replacement draws plain instead.

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
| Home | Moves to the start of the line |
| End | Moves to the end of the line |
| Delete | Deletes the character after the cursor |
| Ctrl-A, Ctrl-E | Moves to the start or the end of the line |
| Ctrl-U, Ctrl-K | Deletes to the start or the end of the line |
| Ctrl-W | Deletes the word before the cursor |
| Alt+←, Alt+→ | Moves back or forward one word (Alt-B and Alt-F do the same) |
| ↑, ↓, or 1 to 9 during a question | Chooses a permission option instead of moving through input or history |
| Ctrl-O | Toggles the pane between folded and whole; works during a question too |
| Tab, Shift-Tab | While the session has subagents, shows the next or the previous view of the bar; while the slash-command list is open, Tab completes instead |
| Escape | Cancels the running turn; in a subagent's view, cancels that subagent's turn where it can be, or goes back to the session's own view otherwise |
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

### Subagents

Some agents start subagents: child agents that do part of the turn, such as
an explorer that reads the repository. Codex ACP and Claude ACP announce
them; OpenCode does not. While the session has at least one subagent, a bar
sits above the status row:

```
  main   ● explorer ?   ✓ reviewer
 agent · develop · claude:opus · running   ⠹ thinking 12s   ⠹ running 1 subagent
```

The bar lists the session's own view, `main`, and then each subagent in the
order the agent started it. The mark before a subagent's title is its state:
`●` running, `✓` completed, `✗` failed, `⊘` cancelled, and `◇` where the
agent has not said. A `?` after a title means that subagent asks a permission
question. The view on the screen is drawn reversed. While any subagent runs,
the status row says `running N subagents`. A session without a subagent has
no bar, and Tab works as before.

Tab shows the next view and Shift-Tab the previous one, in bar order. Each
switch clears the terminal, its scrollback too where the terminal allows,
and draws the chosen view from its start. In the CLI that also clears what
your shell printed above the console.

The `main` view is the session's own conversation. It shows one line when a
subagent starts, with its title and its task, and one line when it ends,
with how long it ran:

```
  ◇ subagent explorer · Find the config loader
  ✓ subagent explorer completed · 1m 04s
```

What a subagent writes and runs is not in the `main` view. Press Tab to read
it in the subagent's own view. That view shows the subagent's transcript
alone, and is read-only: it has no input box, and the footer starts with
`read-only`. Ctrl-O folds and unfolds as in the `main` view. A permission
question from a subagent is asked in the `main` view, since the session
answers it, and names the subagent: `Bash · asked by subagent explorer`.

Escape's footer hint says what it does in that view: `esc cancel` where the
agent said the subagent may be cancelled, and `esc back` otherwise. Not every
agent says so — codex-acp and claude-agent-acp announce none today — so a
subagent's own view usually goes back to `main` on Escape rather than
cancelling.

Over HTTP, `POST /v1/sessions/{id}/console/cancel` takes the same body either
way: omitted, or `{}`, cancels the session's own running turn, and
`{"subagent_id": "<id>"}` cancels that subagent's turn instead, leaving the
session's own turn untouched. It answers `409` for a subagent that is
unknown, has already ended, or was never said to be cancellable.

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
ariadne task logs <task-id> --follow
```

`--tail N` keeps the last N blocks. `--since` accepts RFC 3339 or a duration
such as `10m`. Repeat `--kind` to include several event kinds. These filters
narrow the snapshot, which is the session's 200 newest events rather than
every turn it ever ran; during `--follow`, `--kind` also narrows new events.
`--format json` keeps each daemon event object unchanged for scripts.

Use `ariadne session send <session-id> "Please explain the failure"`
when a script or a one-line response is enough. When a session is waiting on a
permission request, the console shows the call it asks about — its head line,
the rest of the command or the diff under it — and then its choices; pick one
with the arrow keys or its number key. The rest of the command draws in your
terminal's own foreground, not dimmed. See [Permission modes](permissions.md).
When an agent asks you a question, it calls `wait_for_user` with a one-line
hint and the question: its text, the options you can pick, the option it
recommends, and why. The hint is what the Needs attention list shows. The
console draws the question under `waiting for you · <hint>`, with each option
by its number and `(recommended)` and the reason beside the agent's pick:

```
● waiting for you · Pick a base branch
  Which base branch should the task land on?
    1. Main — Land on the trunk. (recommended) · The trunk is green.
    2. Next
  Type a number, or your own answer.
```

Type a number, an option's id or its label to pick that option; the agent
reads the option and what it means. Anything else you type is your own
answer. Your input answers the newest open question alone, and that
question then shows how it closed: `answered · <answer>`, `question
withdrawn` or `question expired`. While a permission request is open, your
input answers the permission and leaves the question open. The daemon
refuses a hint over 100 characters or over more than one line. A later turn
keeps a question only when it calls `wait_for_user` again with its hint.
Otherwise, the turn ending ends the wait.

```
─ permission ───────────────────────────────────
Bash
○ $ cargo build
    cargo nextest run
❯ 1. Allow
  2. Reject
────────────────────────────────────────────────
```

Two rules frame the question. Once you answer, the console keeps only the
question, the call's head line and `↳` with the option you chose.

The desktop app shows a session's console in its detail view, drawn by the
app from the same events. It shows the same blocks: your prompts, the agent's
messages and thoughts, plans, tool calls with their output and diff, and
notes. Type in the input box at the bottom: Enter sends, and Shift+Enter adds
a line. While a turn runs, the Cancel button or Escape in the console cancels
it. A permission request shows its options as a picker: ↑, ↓ or a number key
selects, and Enter or a click answers. The answer then shows the option chosen
and who decided. If the daemon goes away mid-session, the console says it is
reconnecting, and shows the session afresh once it is back. The agent receives
the same input whichever console you use.

In the browser and desktop console, conversation appears beside selected tool
details when the panel has enough space. Narrow panels open a details drawer.
Each tool stays on one compact row with its name, status, and summary.
Click a row, or focus it and press Enter or Space, to read its input, output,
and available code changes. New calls do not change your selection.
Close the details with its close button or Escape to return to the selected row.
The embedded and expanded consoles use the same layout.

Questions stay in the conversation. Permission rows open their command, diff,
and response controls beside the conversation, or in the right drawer on narrow panels.
Use `Pending request` to open a permission and focus its picker. For a question,
this control closes the drawer and reveals the question in the conversation.
This works for subagent requests too.

User, developer and assistant messages always remain expanded. Role labels identify
them. You headers have a person icon, restrained 12% primary background and solid inset border ring. All three role icons share one vertical column.
Assistant headers have a bot icon, subtle muted background and faint solid inset border ring; Developer headers
have a code icon, a slightly deeper shade of the same muted background and solid inset border ring. Both stay fully expanded.
Compact secondary activity sits slightly indented, without left borders.
Pending questions stay open. Answered questions can fold to show the question
and chosen answer together. Thoughts still fold individually. There are no global fold controls in the header or plus menu. Tool rows stay compact. Ctrl-O has no action
in the browser or desktop console; it still works in the terminal console.
Drafts, history, and conversation fold preferences remain available when you
expand the console or change tabs. See [spec 015](../specs/015-desktop-app.md).

Code wraps inside the conversation and details, which scroll independently.
A code block shows its language
when it has one, colors comments, strings, numbers and keywords for the
common languages, and has a `Copy` button that copies the code as written.
Diffs keep their green and red lines. When you scroll up to read, the console
stays where you are, and `Jump to latest` appears, with `New output` once
more has arrived; click it to follow the live output again.

## Work with tasks

```sh
ariadne attention                         # sessions and tasks needing you
ariadne attention dismiss <item-id>       # dismiss one attention occurrence
ariadne task ls --goal <goal-id>          # current tasks
ariadne session ls --attention            # only sessions waiting for input
ariadne task inspect <task-id>            # task, agents, diff, and history
ariadne task messages <task-id> --full    # agent conversation about the task
ariadne task diff <task-id>               # proposed change
```

`ariadne attention` shows only the daemon's shared items, including during incomplete reads.
An incomplete table read keeps its warning; JSON keeps `complete: false`; quiet output exits unsuccessfully.
Dismissal hides one occurrence without answering a request or changing the underlying work.

Use `ariadne attention --columns id` to print an ID table without summaries or actions.
Select `id`, `title`, `reason`, `task`, and `age` in any order, separated by commas.
`title` contains the summary; `task` contains affected task names, or `-` when there are none.
Without `--columns`, the list includes the actions and affected subjects.

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
user's own machine. A task can change its model before it starts:

```sh
ariadne task update <task-id> --model codex-acp:<model-id> --effort high
```

`task update` applies only while a task is pending or ready. How agents'
permission requests are answered belongs to the repository, not the task:

```sh
ariadne repo update <repo-id> --permission-mode ask
```

## Exhausted models and automatic switching

When a model runs out of tokens, reaches a rate limit, or hits another quota
exhaustion, the daemon can switch to another agent or model. Set `auto_switch`
to `false` in the configuration to wait for input instead.

The daemon picks a replacement in this order: another agent at the same rank,
then the same agent at the same rank, then one rank up, then one rank down. For
example, a `balanced` model tries another balanced agent, then itself, then
`frontier`, then `fast`.
When the current model has no rank, or when nothing fits, the daemon flags the
session `exhausted` and waits. OpenCode's silent retries are not detected, so a
token limit there cannot trigger a switch.

Switch to another agent or model yourself at any time:

```sh
ariadne session switch <id> --model codex-acp:<model-id>
```

See [Configuration](configuration.md) and [Resuming a session](resuming-sessions.md)
for the related configuration and session workflows.

## AI permissions

Use `ariadne permissions learned` to inspect and remove the recorded
permission answers:

```sh
ariadne permissions learned list --repo ~/projects/api
ariadne permissions learned show <id>
ariadne permissions learned rm <id>
```

`list` accepts a repository id or path, and shows the tool, the level, the
family, the key, the scope, the permission mode at the time (`target`) and
the selected option. `show` prints every field, the risk tags included, with
the tool call (its original input included), the options and the model
output as formatted JSON. Only a permission decision writes a row, so there is no `add` or
`edit`. Add `--format json` to any verb for JSON output.

`ariadne permissions ai` manages the AI permission model behind the `ai` permission
mode: see [Permission modes](permissions.md#the-ai-permission-model) for what it installs and
what each setting does.

```sh
ariadne permissions ai show                    # settings, hardware, flavours and devices
ariadne permissions ai enable --wait            # turn it on and wait for the install
ariadne permissions ai set --allow-threshold 0.2 --deny-threshold 0.8
ariadne permissions ai set --default-thresholds # back to the flavour's pair
ariadne permissions ai set --flavour 9b --device cuda  # only a combination the machine can run
ariadne permissions ai refresh --wait           # install again, on the settings as they stand
ariadne permissions ai disable                  # keeps the files
```

Each flavour has a default threshold pair, and a pair you never set follows
the flavour ([Permission modes](permissions.md#the-ai-permission-model) lists
them). Each threshold must be from 0 to 1, and the allow threshold must be
lower than the deny threshold. The daemon refuses an invalid pair without
changing either setting. A threshold you set keeps the pair across a flavour
change; `--default-thresholds` goes back to the flavour's pair and cannot be
given with a threshold.

`enable` and `refresh` answer at once with `installing`; `--wait` blocks
until the install leaves that state and exits 1 with the reason on a
`failed` one. A repository refused `ai` before the AI permission model is on, and an `ai_disabled`
error anywhere names the fix: `run ariadne permissions ai enable`. `ariadne
doctor` reports the Python interpreter the AI permission model needs and where its install
stands, next to the rest of the daemon's environment.

## Pull requests, issues and the forge

```sh
ariadne repo add ~/projects/api --forge on
ariadne repo update <repo-id> --forge off
```

`repo add` and `repo update` take `--forge on` or `--forge off` to work with
the forge a repository's remote is on, once `gh` is signed in to
its host. No `--forge` flag sends no change to it. There is no repository
setting that pins a review: every review's model and effort are picked on
the request itself, by `pr review` below. A request a task opens needs no
pin either way: the agent of the task's `pr` column keeps it. See [The
forge integration](forge.md) for what each session does.

```sh
ariadne pr ls --repo <repo-id>
ariadne pr ls --mine
ariadne pr ls --review-requests --watch
ariadne pr search --repo <repo-id> "rate limit"
ariadne pr inspect <repo-id> 42
ariadne pr refresh --repo <repo-id>
ariadne pr review <repo-id> 42 --model claude-acp:<model-id>
ariadne pr review <repo-id> 42 --model claude-acp:<model-id> --skill security-review --attach
ariadne pr review <repo-id> 42 --again
ariadne pr review <repo-id> 42 --stop
ariadne pr babysit <repo-id> 42 --model claude-acp:<model-id>
ariadne pr babysit <repo-id> 42 --stop
```

`ariadne pr ls` lists the open requests from each enabled repository's last
fetch, as the desktop's **All** does. A repository with no fetch yet has no
requests in the list. `--mine` narrows it to
your own, and `--review-requests` to the ones that ask for your review; the
two do not combine. `--repo` narrows it to one repository, and `--watch`
redraws it as requests move. The author column reads "you" on a request of
yours, and the `ariadne` column says whether Ariadne keeps it for a task,
reviews it, babysits it by hand, both of the last two together, or does
nothing with it; `branches` reads `head → base`, and `session` links the
request's newest session.
`ariadne pr inspect <repo> <number>` prints every field of one request,
read off the forge now, as a key-value block in a fixed reading order, the
body last after a `---` line. `ariadne pr search` searches an enabled
repository's last fetched open requests, by number, title or author, your
own left out. `ariadne pr refresh` requests an immediate fetch for one
repository, or every enabled one. It returns after it wakes the fetch, not
after the fetch ends, so a following `pr ls` can still show the previous
list.

`ariadne pr review <repo> <number> --model AGENT:MODEL [--effort E] [--skill
S]... [--attach]` asks Ariadne to review a request of your own (029):
`--model` is required with neither `--stop` nor `--again`, since there is no repository
setting to fall back to, and the model must be ranked `balanced` or
`frontier`, the band of the [`pr-review` workflow](workflows.md#the-pr-review-workflow).
The ask makes one goal with one task; `pr inspect` prints its id as `review
goal`. `--skill` loads another skill beside `pr-reviewer`, which
every review loads anyway; repeat it for more than one. `--attach` waits for
the review session to start and opens its console, with `ariadne attach`. `ariadne pr review <repo> <number> --stop` stops Ariadne's
review and cannot be combined with `--model`, `--effort`, `--skill` or
`--attach`.

`ariadne pr review <repo> <number> --again` requests another round on the current head.
It requires an existing review ask and conflicts with `--model`, `--effort`, `--skill`, `--attach` and `--stop`.
The round runs when the session becomes idle.

`ariadne pr babysit <repo> <number> --model AGENT:MODEL [--effort E]
[--skill S]... [--attach]` asks Ariadne to keep an open request that
no task opened (032), the same way `pr review` asks for a review:
`--model` is required with neither `--stop` nor `--again`, and the model must be ranked `fast`
or `balanced`, the band of the [`pr-babysit`
workflow](workflows.md#the-pr-babysit-workflow). The ask makes one goal with
one task; `pr inspect` prints its id as `babysit goal`. `--skill` loads another
skill beside `pr-babysit`, which every keeper session loads anyway. `--attach`
waits for the keeper session to start and opens its console. `ariadne pr
babysit <repo> <number> --stop` stops it, and cannot be combined with
`--model`, `--effort`, `--skill` or `--attach`. Asking is refused on a
request a task opened.

Beside these two ways to ask for or stop a review or a keeper session,
nothing here adds or removes a request: Ariadne starts and stops its other
work on one on its own (see [The forge integration](forge.md)).

```sh
ariadne issue ls --repo <repo-id>
ariadne issue ls --repo <repo-id> --all
ariadne goal create --from-issue https://github.com/owner/repo/issues/7 \
    --model codex-acp:<model-id>
```

`ariadne issue ls` lists open issues from an enabled repository, assigned to
your forge login by default, or every open one with `--all`. `goal create
--from-issue` reads one issue and uses its title and body to fill the goal.
The orchestrator reads them marked as somebody else's words, never yours,
since the issue could be anybody's; a title and description you type
yourself reach it in plain text.

```sh
ariadne forge tunnel
ariadne forge tunnel on
ariadne forge tunnel off
```

`ariadne forge tunnel` prints the webhook tunnel's switch, state, URL and
bound listener address; `on` and `off` set the switch. See [The forge
integration](forge.md) for enabling a repository's integration, how an
author keeps its request, what a `pr-reviewer` session does, and the tunnel's fallback
to polling; see [Configuration](configuration.md) for `gh_bin`
and the webhook keys.

## Query telemetry

`ariadne query` evaluates a PromQL expression over the daemon's metrics and
prints a table: one column per label of the result, then `VALUE`.

```sh
ariadne query 'sum by (seat) (ariadne_tokens_total)'
ariadne query 'ariadne_tokens_total' --at 2026-01-01T10:00:00Z
ariadne query 'rate(ariadne_tokens_total[5m])' --since 6h --step 1m
```

An instant query runs at now. `--at` moves it to an RFC 3339 time or a span
before now. `--since` runs a range query from a span before now (or an RFC
3339 time) to now, one point per `--step`; the step defaults to the span
over 250, rounded up to a whole second. A range adds a `TIME` column and
prints one row per point. The table flags apply, and `--format json` prints
the daemon's Prometheus answer whole. A refused query prints the daemon's
`error` sentence, such as `unsupported function stddev`. `--host NAME|all` reads
the metrics of one host or of every ready host.

`ariadne stats` is gone. See [Telemetry](telemetry.md#querying-metrics-and-logs)
for the supported PromQL and the metric names.

## Dashboards

`ariadne dashboard` manages Perses dashboard documents. Use `ls`, `get`, `set`, `reset`, and `rm`.

```sh
ariadne dashboard ls
ariadne dashboard get default
ariadne dashboard set mine dashboard.json
ariadne dashboard reset default
ariadne dashboard rm mine
```

## Custom workflows

A workflow is a linear kanban of columns a goal's tasks step through, one
agent per column. Ariadne ships two, `develop-review-merge` and
`develop-review-pr`; `ariadne workflow` manages the catalog the way `ariadne
skill` manages skills — a shipped one is reset rather than deleted, one of
your own is deleted rather than reset. See [Workflows](workflows.md) for the
document syntax and what each gate checks.

```sh
ariadne workflow ls
ariadne workflow show develop-review-merge
ariadne workflow check --file draft.md
ariadne workflow create my-workflow --file draft.md
ariadne workflow update my-workflow --file draft.md
ariadne workflow reset develop-review-merge
ariadne workflow rm my-workflow
```

`create` and `update` read the document from `--file`, or from stdin where
none is named or where `-` names it. `check` parses a draft without saving it anywhere: it prints
the document's columns, or the line a syntax rule broke on. `show` prints
the document whole, through `$PAGER`; `--format json` prints the DTO.

### Running a goal through a workflow

Every goal runs through a workflow. `goal create --workflow <name>` picks
it; left out, the goal takes its first repository's default workflow.
`repo add --workflow <name>` and `repo update --workflow <name>` set that
default:

```sh
ariadne goal create --title "Add rate limiting" --repo ~/projects/api \
    --model codex-acp:<model-id> --workflow develop-review-merge
ariadne repo update <repo-id> --workflow develop-review-merge
```

A task is staffed one agent per column with `--agent
STEP[:SKILLS]=MODEL[@EFFORT]`, repeatable:

```sh
ariadne task create <goal-id> --title "Add the rate limiter" \
    --agent develop:coding=codex-acp:<model-id> \
    --agent review:code-review=claude-acp:<model-id>@high \
    --agent merge=codex-acp:<model-id>
ariadne task update <task-id> --agent develop=codex-acp:<other-model-id>
```

Leaving the `:SKILLS` half out stages the column's own skills. `task update
--agent` replaces the whole staffing.

`task ls` shows each task's current column in a `step` column, and `--step
<id>` narrows the list to it. A task that runs on another host adds a `HOST`
column after the title, which names that host. A host that cannot be reached
shows `(unreachable)` after its name. A list with no such task has no `HOST`
column. `task inspect` prints the workflow, the current
column, and one line per column — its skills, its pin, and its session.
On a worker, inspection uses the task’s saved workflow without a worker goal.
A goal filter can use the saved source-goal ID or its prefix.
With `--host all`, a source projection and worker response appear once.
`task history` paints the column a move left and the one it entered beside
the statuses. `goal inspect` prints the workflow and its columns, each with
its capability band — every rank it lists — and its gate:

```sh
ariadne task ls --step review
ariadne task inspect <task-id>
ariadne goal inspect <goal-id>
```

`attach --step <id>` opens the console of that column's agent, live or not;
left out, it is the task's current column. `--seat agent` names the same
seat every stepped session carries, on `task attach`, `task logs` and
`session ls` alike:

```sh
ariadne task attach <task-id> --step review
ariadne session ls --seat agent
```

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

## Update an SSH worker

Select the worker by name.
Updates use its saved SSH address and socket.

1. Create a JSON request with binary selections, tool selections, or both.
2. Run `ariadne host update builder --request worker-update.json`.
3. Read progress with `ariadne host update-status builder`.

A tools-only request leaves worker binaries in place:

```json
{"tools":{"agents":["codex-acp"],"github":true}}
```

A binary-only request leaves agent programs in place:

```json
{"source":{"kind":"daemon"}}
```

This selects the settings daemon’s own version.
A source daemon builds its commit on the worker before maintenance starts, as setup does.
Omitting `source` updates no Ariadne programs.
Combine both sections to update binaries and tools together.
The source also accepts an explicit verified release, as described under [worker setup](#install-an-ssh-worker).
Local binary paths refer to the settings daemon machine.
Do not include SSH settings, credentials, or repositories in update requests.

The Desktop Hosts screen offers the same selections under **Update** in the row of the machine.
Select **Update Ariadne binaries**, **Update agent tools**, or both.

The current worker and selected replacements must support maintenance.
Ariadne checks the current worker before transfer.
It checks staged replacements before changing installed binaries.
Unsupported versions produce guidance and preserve installed worker binaries.
Updates use Ariadne's own CLI for maintenance requests.
Only a source build on a worker without Rust needs curl.

Maintenance prevents new task starts and lets running tasks finish.
The operation then changes selected programs, restarts the service, and checks health.
Successful maintenance restores scheduling.
Automatic configuration synchronization never updates worker programs.

Selected tool updates require user-owned Homebrew for Git and optional GitHub CLI.
A required Node or uv runtime uses Homebrew, or an install in the user's home directory (see setup below).
An update upgrades a runtime that setup installed there, and leaves one that you installed.
npm packages use the worker user's prefix.
Unsupported programs require manual installation.
No update runs administrator commands.

A failed update retains completed steps and reports the failed step.
Ariadne checks worker health after failure.
A healthy worker resumes scheduling.
Pending configuration maintenance can still delay new task starts.
Failed health checks or maintenance release leave scheduling held and report a separate resume error.

Correct the cause or change the failed selections, then retry.
For example, remove an unsupported tool or select another verified release.
Retries preserve unchanged completed steps and rerun work affected by changed selections.
Running operations refuse selection changes until they finish.

See [the host specification](../specs/037-hosts.md#explicit-worker-updates) for the update contract.

## Install an SSH worker

1. Create a worker account and configure noninteractive SSH authentication and host trust.
2. Inspect the worker with `ariadne host inspect builder --ssh ssh://worker@build-machine:22`.

   Inspection requires no worker tools and changes no worker files.
   It reports the platform, available commands, credential-file presence, and user-service prerequisites.
   Provider login verification runs during setup because provider commands can create files.
   Each prerequisite names the missing capability, its resolution, and the recheck procedure.
   Inspection also lists supported agent packages and credential transfers.
   It prints each agent, and GitHub CLI, under one of three groups:

   - **Ariadne installs**: setup installs the item.
   - **Already present**: the item is on the worker, and nobody installs it.
   - **Manual**: you install the item or sign in by hand on the worker.

   A manual item gives the reason and, where one exists, the command to run on the worker.
   An agent that this daemon can run on its own machine says `on this machine`.
   Inspection lists each sign-in, whether Ariadne can copy it, and the command that signs in on the worker.
   Add `--format json` for the `InspectionDto`, with its `agents`, `github` and `sign_ins` fields.

3. Create a request file on the machine where you run the CLI:

   ```json
   {
     "ssh": "ssh://worker@build-machine:22",
     "source": {
       "kind": "local",
       "cli": "/absolute/path/to/ariadne",
       "daemon": "/absolute/path/to/ariadned"
     }
   }
   ```

   You can also use a request that contains only `ssh`.
   Omission of `source` selects `{"kind":"daemon"}`.
   A release daemon downloads and verifies its own version for the worker’s platform.
   A source daemon builds its own commit on the worker:

   - It packs the commit from the checkout that built it, and sends it through SSH.
   - The worker gets the commit only. Uncommitted edits and untracked files stay on this machine.
   - Setup refuses before any transfer when the checkout or the commit is missing on this machine.
   - The worker needs a C compiler, a linker, the C library headers, and network access to crates.io.
     Setup compiles and links a small C program to check them.
   - When the worker has no Rust that runs, setup installs it with rustup in the user's home directory.
     This needs `curl` on the worker, unless `rustup` is already there.
   - Setup runs no administrator command. Review shows a missing compiler, linker, header, or `curl` under "Manual".
     Each manual item has the install command for the worker's OS.
     Setup stays incomplete until you install the item and run `ariadne host resume`.
   - The worker builds both programs in release mode, in `~/.ariadne/worker-build`.
     A first build takes several minutes. When the build fails, read `build.log` in that directory.
   - A retry uses a completed build of the same commit again.

   Inspection reports the method, version, tag, commit, and uncommitted status in `binaries`.
   See [the host specification](../specs/037-hosts.md#ssh-worker-setup) for the full rules.

   The source daemon machine runs the settings daemon.
   Both `cli` and `daemon` paths refer to files on that machine.
   Both binaries must match the remote OS and architecture.
   For a verified release, replace `source` with:

   ```json
   {"kind": "release", "repository": "owner/ariadne", "tag": "v1.0.0"}
   ```

   Add optional tool and credential selections to the request:

   ```json
   "tools": {"agents": ["codex-acp", "claude-acp"], "github": true},
   "credentials": ["codex", "github"]
   ```

   Use shipped ACP registry IDs for agents. Setup installs their npm or uv packages in user-owned locations.
   It also installs Git, selected GitHub tooling, and required Node/npm or uv runtimes.
   Missing Git and GitHub CLI use an existing user-owned Homebrew installation.
   A missing Node.js or uv uses Homebrew when it is user-owned. Otherwise setup installs it in `~/.local`, with no administrator command:
   Node.js from the current LTS archive on nodejs.org, checked against its `SHASUMS256.txt`, and uv with the installer from astral.sh.
   If `/bin/bash` is missing, the step tells you to install Bash there and resume setup.
   The worker needs `/bin/bash` for runtime process supervision, `curl`, and for Node.js `tar`, and network access to nodejs.org or astral.sh.
   A failed install shows the network access and the command to run by hand.
   Otherwise, setup reports administrator prerequisites without running administrator commands.
   uv can install its required Python runtime.
   Custom commands and unsupported distributions require manual installation.

   Credential selections reference credentials on the source daemon machine.
   `codex` copies its default `.codex/auth.json` through SSH with private file permissions.
   `github` obtains the source GitHub CLI token for `github.com` and sends it through SSH input.
   GitHub CLI stores the worker login and configures Git authentication.
   No selection copies any other credential.
   Keychains, SSH keys, Claude credentials, and OpenCode credentials require separate worker login.
   Credential values never belong in the request file.
   The daemon rejects unknown credential references before it stores an operation.

   Setup checks Codex, Claude, and selected GitHub authentication before completion.
   Other agents lack supported authentication checks and leave setup incomplete.
   Complete their login separately; setup does not automate provider login flows.
   Progress names each tool, credential transfer, and authentication check.
   Retries preserve successful installations and credential transfers. Authentication checks run again.
   npm checks the selected package in the worker user's global installation prefix and preserves installed packages.
   Retry with exactly the original request, including its selections.

   Add selected source repository IDs and worker destinations:

   ```json
   "repositories": {"SOURCE_REPOSITORY_ID": "~/workspace/project"}
   ```

   Find source IDs with `ariadne repo ls --format json`.
   The proposed workspace root is `~/workspace` on the worker.
   Change each destination to an absolute path or a path that starts with `~/`.
   Setup expands `~/` with the worker home from the SSH probe.
   Git clones committed history from the selected repository's remote.
   Local edits and untracked files stay on the source machine.
   Git and forge authentication must work on the worker.
   For an SSH remote, setup copies this machine's `known_hosts` entry for the Git host to the worker, so the worker trusts the key you already trust.
   With no such entry, the clone step names the host and the `ssh -T` command to run once on the worker.
   Setup reports missing remotes, untrusted host keys, authentication failures, missing branches, and existing destinations in the repository steps, by repository name.
   It never replaces an unrelated directory.
   A matching registered forge checkout keeps its existing worker path.
   Matching uses forge kind, host, owner, and name, with the same base branch preferred.
   Completed clones and registrations survive retries.
   Existing settings and learned permissions follow the normal host synchronization.
   After setup completes, submit the same request with changed repository selections to populate more repositories.
   This explicit update preserves existing checkout paths.
   Desktop Settings provides repository checkboxes and editable destinations in the worker setup form.

4. Review the plan with `ariadne host review builder --request worker.json`.

   Review shows installations, selected credential transfers, repository destinations, preserved settings, and prerequisites.
   It prints one plan item for each thing that the request causes, under the same three groups.
   Each manual item gives its reason and, where one exists, the command to run on the worker.
   Review changes no worker files and starts no setup operation.
   Add `--format json` for the `SetupReviewDto`, with its `plan` field.

5. Apply the selections with `ariadne host setup builder --request worker.json`.
6. Read progress with `ariadne host setup-status builder`.

   Progress prints each step with its title, its name, its state and its error.
   Each step is under one of the three groups.
   A step that found its item on the worker and installed nothing is under **Already present**.
   A step that waits for manual work is under **Manual**, with the same reason and command as its plan item.

7. Resolve manual prerequisites on the worker.
8. Run `ariadne host resume builder` to recheck prerequisites and continue the saved operation.

Setup continues independent automatic actions when a selected tool or login needs manual action.
The worker remains incomplete until all selected required actions succeed.
Resume retains completed installations, credential transfers, clones, and registrations.
Desktop provides Inspect worker, Review setup, an explicit apply button, and Recheck and resume setup.
A changed selection requires a new review before application.
Load setup progress by worker name to resume without entering the selections again.

Run `ariadne host setups` to list each saved setup and update with its state and SSH address.
Run `ariadne host discard builder` to delete the saved record of a setup in state `incomplete` or `failed`.
The commands use `GET /v1/host-setups` and `DELETE /v1/hosts/{name}/setup`.
Discard changes no file on the worker. It refuses a setup that is `pending`, `running` or `complete`, and it never discards an update.
A new setup with the same name then starts from the first step.

Setup uses existing SSH keys, agent access, and SSH configuration without password prompts.
Inspection reports missing Git and tar. Install prerequisites manually when no supported automatic installation is available.
Release verification requires GitHub CLI 2.102.0 or later on the source daemon machine.
An administrator must provide missing system packages or user service support.
Linux workers need systemd user services and lingering for operation after logout.

Setup preserves worker data, configuration, existing service definitions, and completed installation steps.
It installs binaries in `~/.local/bin` and registers the platform's user service.
For new services, setup captures the remote user's PATH through a noninteractive login shell and prepends `~/.local/bin`.
Setup uses only PATH text between probe markers and ignores login-profile banners and other output.
If the shell fails, markers are missing, or the marked PATH is empty, setup uses the SSH session's PATH.
A PATH with control characters stops setup before transfer.
Existing service files retain their PATH and other machine-specific settings through retries.
Update an existing service file yourself if its PATH lacks required tools.
A custom remote daemon socket needs the request's optional absolute `remote` path.
The default socket is `~/.ariadne/ariadne.sock` on the worker.
A failed or incomplete status returns a failure exit code.
A complete status requires the daemon to answer and the connection to be registered.
Use `--format json` for operation state and per-step results.

The Desktop Hosts screen provides the same steps in its **Add machine** dialog.
Select **Install Ariadne on a machine through SSH**, then follow the steps Connect, Select, Review and Install.
Select **Connect to a daemon that already runs** for the same change as `host add`.
Each row has **Edit** and **Remove**, for the same changes as `host edit` and `host rm`.
Remove does not stop the daemon on the host.
Select **Check again** on the Install step to resume a stopped setup with its original request.
A failed tool other than Git, a credential copy or a sign-in check also has **Skip**.
A skipped step keeps its instructions, and **Continue setup** goes on without it, so the machine can connect.
Install the skipped item by hand later, or select it again in an update.
Agent installation, credential transfer, and repository cloning are separate operations.
See [the setup contract](../specs/037-hosts.md#ssh-worker-setup).

## Selecting a host

Without `--host`, commands use the resolved daemon endpoint.

Use `ariadne host ls` to list configured hosts. The columns are `name`, `kind`, `status`, `endpoint`, `remote`, `sync`, `missing`, `skipped`, `error` and `sync_error`.
The `sync` column is `in_sync`, `pending`, `pending_restart` or `failed`.
`pending_restart` means the worker waits for admitted tasks before applying registry changes. `missing` counts the repositories the host lacks. `skipped` counts the learned permissions not pushed. `remote` is the far address of an `ssh` host. `sync_error` is why the last push failed. `--columns` selects them, and `--format json` prints the host objects.
Use `ariadne host reconnect NAME` to reconnect one named host, or `ariadne host reconnect all` for each named host.
The command waits until each host is `ready` or `down`, up to `--timeout SECONDS` (default 30).
It then prints one row per host, with the columns of `host ls`; `--format json` prints the hosts.
It exits 1 when a host is not `ready` at the end. `local` cannot be reconnected, and `--host` does not apply to it.
Use `ariadne host add NAME --endpoint ENDPOINT [--remote REMOTE] [--color COLOR]` to add a host, and print its row.
Use `ariadne host edit NAME` with `--endpoint`, `--remote`, `--no-remote`, `--color` or `--no-color` to change it. Only the flags you give change, and you must give one.
Use `ariadne host rm NAME` to remove a host and print its name.
`--color` takes `red`, `orange`, `amber`, `green`, `teal`, `blue`, `violet` or `pink`. On these commands, `--endpoint` and `--color` belong to the host. Put the global `--endpoint` before `host`.
They run on the local daemon, and `--format json` prints the host.
Use `--host NAME` to run a command on a ready host from the local daemon's registry.
`--endpoint` still selects the local daemon that supplies this registry.
`--host local` selects that same daemon.

Use `--host all` for reads only.
A list across hosts is one list, read from every ready host at the same time.
It looks like the same list without `--host`, with a leading `HOST` column.
A task list also shows the host of each task in a `TASK HOST` column, when one of its tasks runs on another host.
The columns line up across hosts, and `--columns`, `--no-trunc` and `-q` work as usual.
Rows follow registry order. A summary line, such as `3 of 12 sessions`, counts every host.
`attention --host all` keeps its bullets and names the host of each item.
With `--format json`, the JSON keeps its form, and each item has a `host` field.
In `session ls` JSON, `total` counts every host and `next_cursor` holds one cursor for each host.
A hint, such as `Next:`, starts with its host and adds `--host NAME`.
To continue with `--cursor`, select one host with `--host NAME`. `--limit` applies to each host.
A host that lacks the ID of a filter, such as `task ls --goal G`, adds no rows.
When no host has that ID, the command fails as not found.
A host that is down, has no endpoint or fails produces a warning.
One successful host gives exit code 0.
With `--watch --host all`, goal, task, session and PR lists redraw one merged table after host changes.
`attention --watch --host all` redraws its merged view too.
`events -f --host all` follows every ready host and labels each line with its host.
JSON event lines have a `host` field.
`daemon logs -f --host all` labels each log line with its host.
Task and session logs find the host with the ID before following it.
An unavailable host produces one warning. Other hosts continue after a host drops, without reconnecting to it.
Ctrl-C stops all followers. These forms also work when the registry contains only `local`.
A command for one ID, such as `task inspect`, finds the host that has the ID.
Its first line is `host: NAME`, and in JSON the answer has a `host` field.
When the ID or its prefix matches on two hosts, the command is refused, and the message names both hosts.
`no host has that ID` prints only when each ready host answered not found. A host that fails prints its own error as a warning.
`attach`, `goal attach` and `task attach` find the host in the same way.
A read that every host answers prints one block for each host, headed by the host name.
These reads are `version`, `daemon status`, `doctor`, `models show`, `skill get`, `skill inspect`, `workflow show`, `permissions ai show`, `permissions ai test`, `forge tunnel`, `workflow check` and `completions`.
With `--format json`, such a read prints an array, and each item has a `host` field.
`daemon logs --host all` prints the recent lines of each host, each led by the host name.
A host that is down or fails prints one warning on stderr, and the other hosts still print.

Writes run on one host. With `--host all`, a write is refused, and the message names the hosts that take it.
Skills, workflows, permissions and repository settings are written on `local`.
The local daemon syncs these settings to every host.
On a named host, these commands write the host's own settings:

- `repo add PATH [--branch BRANCH]`, `repo update ID --path PATH` and `repo rm ID`.
  A synced flag, such as `--description` or `--workflow`, is refused on a named host.
- `permissions ai set --device` alone. It stays until `local` changes the flavour; see [Choosing a flavour and device](permissions.md#choosing-a-flavour-and-device).

Some commands run on `local` only, and refuse `--host NAME` and `--host all`:

- `host ls`, `host setup`, `host setup-status`, `host setups`, `host discard`, `host update`, and `host update-status`, which use the local daemon.
- `daemon start`, `daemon stop` and `daemon restart`. `daemon status` and `daemon logs` accept a host name.
- `completions install`, which writes the shell configuration of this machine.

An unknown host name gets a message that names the configured hosts.
A host that is not ready gets a message with its status and its error.
`daemon status --host NAME` shows only that host's health and version, with no service manager or TCP setting of this machine.
`daemon logs --host NAME` prints that host's log. When the host does not answer, the command fails with the reason and never prints the local log file.
`daemon --home PATH status --host local` and `daemon --home PATH logs --host local` use the daemon of that home.
`query` accepts a host name or `all`. An unknown host, or one that is not ready, ends with exit code 2, as for every command. With `all` the table has `HOST` as its first column, and a host that did not answer prints `warning: <host>: <reason>` on stderr.
Shell completion offers configured names and `all`.
A `--host` value that looks like an address gets a message that names `--endpoint`.
Addresses formerly passed through `--host` must use `--endpoint`.

See [Addressing another daemon](configuration.md#addressing-another-daemon) for endpoint resolution.

## Command reference

All commands accept `--endpoint`. Use [`--host`](#selecting-a-host) to select a configured host.
Table commands also accept `--format`,
`--color`, `--no-trunc`, `-q`, `--output`, and `--columns`; long output
accepts `--no-pager`. Use `ariadne help [COMMAND]` or `COMMAND --help` for
the built-in help screen.

| Group | Commands and options |
| --- | --- |
| `host` | `ls` lists configured hosts and their connection status. |
| `version`, `doctor` | No command-specific options. |
| `daemon` | `start`, `stop [--timeout SECONDS]`, `restart [--timeout SECONDS]`, `status`, and `logs [-f]`; each accepts `--home PATH`. |
| `agent` | `ls`, `refresh`, and `update AGENT [--flag FLAG]... [--clear-flags\|--reset]`. |
| `models` | `ls`, `show MODEL`, `enable MODEL`, `disable MODEL`, `rank MODEL RANK\|--clear`. |
| `skill` | `ls`, `get NAME`, `inspect NAME`, `create NAME [--file FILE]`, `set NAME [--file FILE]`, `reset NAME [-y]`, `rm NAME [-y]`. |
| `workflow` | `ls`, `show NAME`, `check [--file FILE]`, `create NAME [--file FILE]`, `update NAME [--file FILE]`, `reset NAME [-y]`, `rm NAME [-y]`. |
| `repo` | `ls`, `inspect ID`, `add PATH`, `update ID`, `rm ID [-y]`. Add and update use `--branch`, `--description`, `--permission-mode auto\|ask\|learn\|ai`, `--workflow`, and `--forge on\|off`; update also uses `--path`. |
| `goal` | `create --model AGENT:MODEL [--title] [-d\|--description] [--from-issue] [--repo]... [--effort] [--workflow]`; `ls [--status STATUS,...] [-a] [--watch]`; `inspect`, `complete`, `cancel [-y]`, `rm [-y]`, and `attach` take an ID. |
| `task` | `create GOAL --title TEXT --agent STEP[:SKILLS]=MODEL[@EFFORT] [--description] [--depends-on]... [--repo]`; `update ID` accepts `--title`, `--description`, repeatable `--agent` and `--depends-on`, or `--clear-depends-on`; `ls` accepts `--goal`, repeatable `--status`, `-a`, `--step`, and `--watch`; `inspect`, `messages [--full]`, `history`, `cancel [-y]`, `retry`, `diff`, `attach [--seat] [--step]`, and `logs [--seat] [-f] [--tail] [--since] [--kind]...` take an ID. |
| `session` | `ls` accepts `--agent`, `--task`, `--goal`, `--pull-request`, repeatable `--status`, `--seat`, `--attention`, `--dir`, `--since`, `--until`, `--search`, `--limit`, `--cursor`, `-a`, and `--watch`; `switch ID --model AGENT:MODEL [--effort]`; `inspect ID`; `send ID TEXT`; `logs ID [-f] [--tail] [--since] [--kind]...`; `resume ID`; `kill ID [-y]`. |
| `events`, `attention`, `attach` | `events [-f] [--goal] [--task] [--session] [--kind]...`; `attention [--watch]`; `attach ID [--seat orchestrator\|agent\|reviewer\|keeper]`. |
| `query` | `query EXPR [--at RFC3339\|SPAN \| --since RFC3339\|SPAN [--step SPAN]]`; evaluates the supported PromQL subset at now, at `--at`, or as a range from `--since`. |
| `pr`, `issue`, `forge` | `pr ls [--repo] [--mine\|--review-requests] [--watch]`, `inspect REPO NUMBER`, `search --repo REPO QUERY`, `refresh [--repo]`, `review REPO NUMBER [--model] [--effort] [--skill]... [--attach\|--stop\|--again]`, `babysit REPO NUMBER [--model] [--effort] [--skill]... [--attach\|--stop]`; `issue ls --repo REPO [--all]`; `forge tunnel [on\|off]`. |
| `permissions` | `ai show`, `enable [--wait]`, `disable`, `refresh [--wait]`, `set` with `--allow-threshold`, `--deny-threshold`, `--default-thresholds`, `--flavour`, or `--device`, and `test --tool TEXT --input JSON [--kind] [--option]... [--location]... [--workspace]`; `learned list [--repo]`, `show ID`, `rm ID`, and `scope ID all\|repository`. |
| `completions`, `mcp` | `completions SHELL [--static]` for bash, elvish, fish, PowerShell, or zsh; `completions install [--shell bash\|zsh\|fish]`; `mcp serve`. |

To wake an existing keeper immediately, run `ariadne pr babysit <repo> <number> --again`.
This works on requests a task keeps. It sends untold news or asks the keeper to read the request again.
Do not combine `--again` with `--model`, `--effort`, `--skill`, `--attach` or `--stop`.
The daemon decides readiness and promotes checked drafts. Sessions make no readiness report.

## Run a standalone agent relay

Use the `relay` subcommand of the installed `ariadned` binary.
Start one ACP agent with a session ID:

```sh
ariadned relay --session example --agent codex --task task1 -- codex-acp
```

Use `--home PATH` to select another Ariadne home.
The pane shows session status and an attach command without a daemon connection.
The daemon does not use this relay yet.
The displayed attach command will reach it after daemon integration lands.
Use Ctrl-C to stop the agent group and remove the session files.
An upgrade leaves each running relay on its existing code until its session stops.
Future daemon integration must use the versioned greeting to preserve compatibility with older relays.

Integrators can use the socket and journal contracts in
[spec 038](../specs/038-agent-relay.md).

A successful `ariadne task retry` prints the returned task status. For a remote
task, the source can still report `failed` while delivery is queued; the status
changes when the worker acknowledges recovery. See [the API contract](../specs/012-http-api-events-and-usage.md).
