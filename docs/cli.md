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
task id or a goal id from the table above. A session that has ended is
revived first, and then its console opens. Leave the console with Ctrl-C
twice or Ctrl-D.

```sh
ariadne attach <goal-id>                 # the orchestrator
ariadne attach <task-id>                 # the agent of the task's current column
ariadne task attach <task-id> --step review   # one column's agent instead
ariadne attach <session-id>              # one specific session, live or ended
```

In a terminal the console is full screen. It takes the alternate screen of
the terminal, and it gives the screen back when you leave. Your shell and its
scrollback stay as they were. The screen has these parts, from the top:

- The transcript: each turn of the session, oldest at the top.
- The plan line, while the agent has a plan with an open entry.
- A permission or a question that waits for you.
- The input box.
- The bottom row, with the state of the session and `? keys`.

```
❯ Run the tests
  ▸ ✓ 12s · 4 steps · 1 file changed
  All 345 tests pass.

❯ Fix the lint
  ▸ ⠹ 4s · 2 steps
    ◐ $ cargo clippy                                       2s
▸ plan 1/3 · Fix the lint
──────────────────────────────────────────────────────────────
❯ Tell the agent what to do
──────────────────────────────────────────────────────────────
 ◐ · Add the frobnicator · agent · develop · Opus 5.5   ? keys
```

### Turns

A turn shows your prompt after `❯`, one activity line, and the answer of the
agent. The activity line has a fold mark (`▸` closed, `▾` open), the state mark
of the turn, how long it took, then its counts: steps first, then changed files,
failed, cancelled and subagents, each only above zero. A prompt the daemon sent
shows its first line with text after `» daemon`, without its Markdown mark.
Move the cursor to that line and press Enter to read the whole prompt as
Markdown, and Left to close it. The answer is markdown, and it streams while the
turn runs. A turn that runs and is closed shows its last three steps, live.

A warning note and a compaction always show under the activity line of
their turn, also while the turn is closed. Each one is a row the cursor can
reach: open it to read its whole text. Where a turn has more of them than it
carries, one row gives the count: `2 more notices in the steps of the turn`.
Open the turn to read them as its steps. Errors show under a closed turn
too. An info note shows only when you open the turn.

An image, an audio block, a resource and a resource link show as one line
with their type and their name, such as `[image] shot.png` or
`[resource link] The spec`.

### Steps

Open a turn to see a row for each of its steps. A row has the status mark
(`○` pending, `◐` running, `✓` done, `✗` failed, `⊘` cancelled), the icon of
its kind, and the first line of its title. The subject follows the title only
where the title does not contain it. The tail of each row is in one column at
the right edge: the permission mark, the `+N -N` of an edit, then how long the
step took. A permission that was answered is a mark on the row of its call, and
it has no row of its own. No mark shows for an allow that `auto` or a learned
permission gave. A dim `✓` is an allow by the AI or by you, and a red `✗` is a
rejection. The message step that is the answer of the turn is no row: the
answer shows under the turn.

Open a step to see its detail in place, under its row. The console reads it
from the daemon. What it shows depends on the kind of the call:

- A command shows `$ <command>` only where its row does not show the whole command, then its working
  directory, its output and its exit code. The output keeps its last 12 lines.
- An edit shows each file as a diff: the file name, then the hunks with the
  added lines green and the removed lines red. Where a hunk replaces as many
  lines as it removes, the words that changed are bold. The diff keeps its
  first 24 lines.
- A read shows the path and the line, then the file text, coloured by the
  grammar of its language. The text keeps its first 24 lines.
- A search and a fetch show what they looked for and what they found.

A fold shows the count of the lines it left out, such as `… 18 more lines`.
Press Enter again on the open step to see all of it. Press `r` on a tool call
to see its raw input and its raw output, as JSON. Press `r` again to see the
detail again. The detail of a call that had a permission ends with the answer
and who decided: `permission: Allow · by ai`, then the reason of the AI and
the learned rule where there are any.

```
    ✓ $ cargo nextest run                                  ✓ 8s
      … 41 more lines
          Summary [   7.910s] 345 tests run: 345 passed, 0 skipped
      exit 0
```

### Subagents

Some agents start subagents: child agents that do part of the turn, such as
an explorer that reads the repository. Codex ACP and Claude ACP announce
them; OpenCode does not.

While the session has a subagent, the top row of the screen is a bar of
tabs: `main`, then one tab for each subagent, in the order they started.
Each subagent tab has the state mark of the subagent and its name. The
active tab is in brackets and reversed:

```
[main] ✓ Explore  ◐ Review
```

In a narrow terminal, the bar cuts the names and keeps the marks. Where the
tabs do not fit, `‹` and `›` show that more tabs are past the edge.

In `main`, a subagent is one row in the turn that started it: its state
mark, `↳`, its name, its step count and its time. The row does not open in
place:

```
    ✓ ↳ Explore  12 steps · 1m 04s
```

Press Enter on the row to open the tab of the subagent. The tab shows the
task of the subagent and a row for each of its steps, the same rows as an
open turn shows. Press Ctrl-N for the next tab and Ctrl-P for the tab
before. Press Escape in a subagent tab to go back to `main`. Each tab keeps
its own scroll and its own cursor. The input box sends to the session from
every tab.

Open one of those steps to see its detail. A permission from a subagent
shows above the input with the name of the subagent:
`Bash · asked by subagent explorer`.

### The plan

While the agent has a plan with an entry that is not done, one line shows
above the input: `▸ plan 2/5 · <the entry in progress>`. Press Ctrl-T, or
click the line, to open the checklist: `☐` pending, `◐` in progress, `☑`
done. Press Ctrl-T again to close it. A checklist that does not fit shows a
part of its entries, and a last row such as `0 above · 23 below · the wheel
scrolls the plan`. Turn the wheel over the checklist to see the other
entries. One notch moves no more lines than the checklist shows, so no
entry is skipped.

### The bottom row

One row sits under the input box. On the left it shows the mark of the
session state (`◐` running, `●` idle, `✗` failed, `■` ended), the name of
the session, the model, the mode, how much of the context window it uses, and the cost. An item that the
agent does not report is not shown. On a narrow terminal the row drops whole
items, the least important first: the name, the cost, the context, the mode,
the model and the mark. The name is the title and the seat that `ariadne
session ls` shows, with the step where the session has one; a name that is too
long is cut with an ellipsis. While the console is open it sets the title of
the terminal window to the name, and it sets the title back when you leave. While the link is down the left side reads `reconnecting`, and
after one Ctrl-C it reads `ctrl-c again to leave`.

On the right the row shows `? keys`, which drops last. Press `?` with an empty
input box, or in the transcript zone, to open a list of every key, in a group
for each zone. On a short terminal, scroll the list with the arrows, PgUp, PgDn, Home and End. `?` or Escape closes it, and the transcript and the input stay
as they were. A `?` in a box that holds text is a typed character.

### The two zones

The keys go to one zone at a time. The input zone has them when the console
opens. Tab gives them to the transcript, with the cursor on the newest turn.
Tab, Escape, or a typed character gives them back to the input. The line of
the item at the cursor is reversed. `?` lists the keys of each zone.

| Key, in the transcript | What it does |
| --- | --- |
| ↑, ↓, `k`, `j` | Moves the cursor between the turns and the rows of the open ones |
| Enter, → | Opens the item at the cursor; on an open step, shows its detail whole |
| ← | Closes the open item, or moves the cursor from a step to its turn |
| `r` | Shows the raw input and raw output of the tool call at the cursor |
| `y` | Copies the item at the cursor as `ariadne session logs` prints it, to the clipboard of the terminal (OSC 52) |
| Home, End | Moves the cursor to the first or the newest turn |
| Enter, on a subagent row | Opens the tab of the subagent |

The wheel scrolls the transcript three lines, in both zones. Over an open
checklist of the plan, it scrolls the checklist instead. A click on a
turn or a step opens or closes it, and moves the cursor there. A transcript
scrolled to its end follows the new lines. A resize draws the whole screen
again at the new size, and loses no line.

The input box has a dim rule above and below it. Its prompt is `❯ `, and
continued rows align under the text. An empty box says `Tell the agent what
to do` in dim text. The box grows to four text rows, then scrolls with the
cursor.

| Key, in the input | What it does |
| --- | --- |
| Enter | Sends what you typed, or answers the permission question on screen |
| Shift+Enter, Alt+Enter, Ctrl-J | Starts a new line without sending. Shift+Enter needs a terminal that reports it through the kitty keyboard protocol; Alt+Enter and Ctrl-J work in every terminal |
| `\` then Enter | Removes the final backslash and starts a new line without sending |
| ↑, ↓ | Moves by a wrapped row; at the first or last row, moves through prompt history |
| Home, End, Ctrl-A, Ctrl-E | Moves to the start or the end of the line |
| Delete | Deletes the character after the cursor |
| Ctrl-U, Ctrl-K | Deletes to the start or the end of the line |
| Ctrl-W | Deletes the word before the cursor |
| Alt+←, Alt+→ | Moves back or forward one word (Alt-B and Alt-F do the same) |
| ↑, ↓, or 1 to 9 during a permission | Chooses a permission option instead of moving through input or history |
| PgUp, PgDn | Scrolls the transcript a screen, in both zones |
| Ctrl-O | Opens or closes the newest turn |
| Ctrl-T | Opens or closes the checklist of the plan |
| Ctrl-N, Ctrl-P | Moves to the next tab or to the tab before, in both zones |
| Tab | Gives the keys to the transcript; while the slash-command list is open, completes the pick; in a form, goes to the next question |
| Escape | Cancels the running turn; in a form, declines the question; in a subagent tab, goes back to `main` |
| Ctrl-C twice, Ctrl-D | Leaves the console; the session keeps running, and the console says how to attach again |

Pasting puts the text into the input box where the cursor is, line breaks
and all; nothing is sent until you press Enter. Wide characters and emoji
take the two columns they draw on, in the transcript and in the box alike.
Moving down past the newest history entry restores the draft you were typing.
History includes prompts typed at a console, never prompts sent by the
daemon.

A typed prompt shows as `❯ text` straight away. Until a turn holds it, it
carries a dim `queued` tag: a prompt typed while a turn runs waits for that
turn to end. A prompt the daemon refuses leaves the queue, and the reason
shows under the transcript after `✗`. If the daemon's stream drops, the
footer says `reconnecting` until it is back, and nothing already on screen
is shown twice.

When you leave, the console gives the screen back and prints the turns it
showed, folded, as `ariadne session logs` prints them: the newest 20 at
most, and then the command that prints every turn with its steps. When the
session ends, the console closes and says how to revive the session with
`ariadne session resume`.

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

### Cancel a subagent over HTTP

Over HTTP, `POST /v1/sessions/{id}/console/cancel` takes the same body either
way: omitted, or `{}`, cancels the session's own running turn, and
`{"subagent_id": "<id>"}` cancels that subagent's turn instead, leaving the
session's own turn untouched. It answers `409` for a subagent that is
unknown, has already ended, or was never said to be cancellable.

With stdin or stdout redirected there is no screen to draw: `ariadne attach`
then prints one `kind · summary` line for each finished item of the session:
a prompt, a step when it is done, and a turn when it ends. A permission that
waits prints its options with numbers. The console reads one prompt in each
line, and a number answers the waiting permission. That is what a script
wants.

Use `ariadne session logs <session-id>` to read without sending input. It
prints each turn of the session in three parts: its prompt, one activity line
and its answer. The activity line gives the state of the turn and its counts:
steps, changed files, failed and cancelled calls and subagents. A count of
zero is left out. Under the prompt, the turn shows each permission and question that
waits, and its errors. When the turn has more of these than it shows, a line
gives their count. `--steps` prints each step of the turn under it in place
of those lines: messages, thoughts, tool calls with their output preview,
permissions, questions, notes, errors, subagents and compactions. The task
form finds the session for you:

```sh
ariadne session logs <session-id> --tail 20
ariadne session logs <session-id> --steps --since 10m --kind tool_call
ariadne task logs <task-id> --follow
```

`--tail N` keeps the last N turns. `--since` accepts RFC 3339 or a duration
such as `10m`, and keeps the turns that started, ended or still run after
it, and the steps after it. Repeat `--kind` to keep several step kinds:
`message`, `thought`, `tool_call`, `permission`, `question`, `note`,
`error`, `subagent` or `compaction`. `--format json` prints one typed turn in
each line, as `GET /v1/sessions/{id}/trajectory` gives it. With `--steps`,
each turn carries all of its steps.

`--follow` prints the turns that the filters keep, and then each new item
one time when it is done. A turn prints its prompt when it starts, and its
activity line and answer when it ends. A lost stream reconnects. The follow
then reads the turns that started and ended while the stream was down, and
the answers to permissions and questions that came in that time. It prints
only what was not printed. An answer that comes after its turn ended prints
under a line that names that turn by its time and prompt. With `--format json`, a follow prints one
typed turn in each line when the turn ends, with all its steps when you give
`--steps`.

Use `ariadne session send <session-id> "Please explain the failure"`
when a script or a one-line response is enough. When a session is waiting on a
permission request, the console shows it above the input: the question, the
call it asks about, why the AI permission model had a part where it did, and
then its choices. Pick one with the arrow keys or its number key, and press
Enter. See [Permission modes](permissions.md).
When an agent asks you a question, it uses the question tool of its own
client: the question, the options you can pick, the option it recommends
first, and why. The console draws the question as a form above the input,
one field at a time, under a `question` rule:

```
─ question ─────────────────────────────────────
Pick a base branch
❯ 1/1 Which base branch should the task land on? · pick one
  ❯ 1. Main (Recommended) — Land on the trunk. The trunk is green.
    2. Next
────────────────────────────────────────────────
```

Pick a choice with the arrows or its number key, check several with Space,
and press Enter to take a field. On a text field, type the answer in the
box. The last Enter sends the answer, and Esc declines the question. Text you
type on a choice field is a prompt: it waits for the turn to end and leaves
the question open. While a permission request is open, your input answers
the permission and leaves the question open.

```
─ permission ───────────────────────────────────
❯ Allow Bash?
  Bash · cargo build
  ❯ 1. Allow
    2. Reject
────────────────────────────────────────────────
```

Two rules frame the question. Once you answer, the permission leaves the
space above the input, and the option you chose is a mark on the row of its
call.

The desktop app shows a session's console in its detail view, drawn by the
app as turns. A turn is your prompt, one activity line, and the agent's
answer. The activity line gives the time of the turn, its steps, the files it
changed and its failed calls. Click it to read the steps of the turn. A step
opens under its row to show its input, output and code changes, and `Open`
shows the whole step beside the conversation, or in a drawer on a narrow
panel. A running turn shows its last three steps and streams its answer.

Type in the input box at the bottom: Enter sends, and Shift+Enter adds a
line. While a turn runs, the Stop button or Escape in the console cancels it.
A permission request is a card above the input: ↑, ↓ or a number key selects
an option, and Enter or a click answers. The answered call shows the option
chosen and who decided. A question the agent asks is a form card inside the
turn that asked it. A warning note and a compaction always show in their turn. If the
daemon goes away mid-session, the console says it is reconnecting, and shows
the session afresh once it is back. The agent receives the same input
whichever console you use.

A plan with an open entry shows as one line above the input, and the status
row shows the session's status, mode, context use, cost and tokens. Scroll to
the top to load older turns. When you scroll up to read, the console stays
where you are, and `Jump to latest` appears, with `New output` once more has
arrived; click it to follow the live output again. Ctrl-O has no action in the
browser or desktop console; it still works in the terminal console. Drafts,
open turns and your place remain when you expand the console or change tabs.
See [spec 015](../specs/015-desktop-app.md).

Code wraps inside the conversation. A code block shows its language when it
has one, colors comments, strings, numbers and keywords for the common
languages, and has a `Copy` button that copies the code as written. Diffs
keep their green and red lines.

## Work with tasks

```sh
ariadne attention                         # sessions and tasks needing you
ariadne attention dismiss <item-id>       # dismiss one attention occurrence
ariadne task ls --goal <goal-id>          # current tasks
ariadne session ls --attention            # only sessions waiting for input
ariadne task inspect <task-id>            # task, agents, diff, and history
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
`frontier`, the band of the [fixed `review` column](workflows.md#pull-request-reviews-and-babysits).
The ask makes one task with no goal; `pr inspect` prints its id as `review
task`. `--skill` loads another skill beside `pr-reviewer`, which
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
`--model` is required with neither `--stop` nor `--again`, and the model must be ranked `balanced`
or `frontier`, the band of the [fixed `babysit`
column](workflows.md#pull-request-reviews-and-babysits). The ask makes one task with
no goal; `pr inspect` prints its id as `babysit task`. `--skill` loads another
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

A task needs no goal. Leave the goal out and give `--repo`, a repository id or
registered path. `--workflow NAME` is optional; `--host NAME` names a ready
host, and the task runs on this machine without it. `--depends-on` is refused,
since no other task is there to wait for. With a workflow, `--agent` staffs each
column as above. Without one, give exactly one `--agent` with no column:

```sh
ariadne task create --title "Bump the lockfile" --repo ~/projects/api \
    --agent :coding=codex-acp:<model-id>@high --host build
ariadne task ls --no-goal
```

New tasks assigned to a remote host become blocked worker drafts during planning.
The source goal keeps their references and cached progress. The worker owns their execution and staffing.
Execution starts after plan finalization and dependency completion across hosts.
Both daemons use the same task ID. The worker needs no corresponding goal.
Older remote mirrors and linked goals keep their existing lifecycle.

Use the source goal's task commands to inspect and control this work.
Disconnected tasks retain acknowledged progress and show `(unreachable)`.
Cancellation remains pending until the worker confirms its outcome. A completion that wins cancellation stays finished.
The worker records dependency failures. An explicit source retry restores a blocked draft on the same task ID.
Execution still waits for source approval and completed dependencies.
The source retains cancellation coordination after goal deletion until the worker answers.
Workers need `remote-tasks-v1` and the `source-dependency-controls-v1` capability.
An incompatible worker produces an upgrade instruction before task creation.

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
| `task` | `create GOAL --title TEXT --agent STEP[:SKILLS]=MODEL[@EFFORT] [--description] [--depends-on]... [--repo]`; `update ID` accepts `--title`, `--description`, repeatable `--agent` and `--depends-on`, or `--clear-depends-on`; `ls` accepts `--goal`, repeatable `--status`, `-a`, `--step`, and `--watch`; `inspect`, `history`, `cancel [-y]`, `retry`, `diff`, `attach [--seat] [--step]`, and `logs [--seat] [-f] [--steps] [--tail] [--since] [--kind]...` take an ID. |
| `session` | `ls` accepts `--agent`, `--task`, `--goal`, `--pull-request`, repeatable `--status`, `--seat`, `--attention`, `--dir`, `--since`, `--until`, `--search`, `--limit`, `--cursor`, `-a`, and `--watch`; `switch ID --model AGENT:MODEL [--effort]`; `inspect ID`; `send ID TEXT`; `logs ID [-f] [--steps] [--tail] [--since] [--kind]...`; `resume ID`; `kill ID [-y]`. |
| `events`, `attention`, `attach` | `events [-f] [--goal] [--task] [--session] [--kind]...`; `attention [--watch]`; `attach ID [--seat orchestrator\|agent\|reviewer\|keeper]`. |
| `query` | `query EXPR [--at RFC3339\|SPAN \| --since RFC3339\|SPAN [--step SPAN]]`; evaluates the supported PromQL subset at now, at `--at`, or as a range from `--since`. |
| `pr`, `issue`, `forge` | `pr ls [--repo] [--mine\|--review-requests] [--watch]`, `inspect REPO NUMBER`, `search --repo REPO QUERY`, `refresh [--repo]`, `review REPO NUMBER [--model] [--effort] [--skill]... [--attach\|--stop\|--again]`, `babysit REPO NUMBER [--model] [--effort] [--skill]... [--attach\|--stop]`; `issue ls --repo REPO [--all]`; `forge tunnel [on\|off]`. |
| `permissions` | `ai show`, `enable [--wait]`, `disable`, `refresh [--wait]`, `set` with `--allow-threshold`, `--deny-threshold`, `--default-thresholds`, `--flavour`, or `--device`, and `test --tool TEXT --input JSON [--kind] [--option]... [--location]... [--workspace]`; `learned list [--repo]`, `show ID`, `rm ID`, and `scope ID all\|repository`. |
| `completions`, `mcp` | `completions SHELL [--static]` for bash, elvish, fish, PowerShell, or zsh; `completions install [--shell bash\|zsh\|fish]`; `mcp serve`. |

A successful `ariadne task retry` prints the returned task status. For a remote
task, the source can still report `failed` while delivery is queued; the status
changes when the worker acknowledges recovery. See [the API contract](../specs/012-http-api-events-and-usage.md).

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
