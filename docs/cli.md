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
prompt. The agent keeps working after you leave with Ctrl-C.

```sh
ariadne attach <goal-id>                 # the orchestrator
ariadne attach <task-id>                 # the task's author
ariadne attach <task-id> --seat reviewer # that task's reviewer
ariadne attach <session-id>              # one specific session
```

Use `ariadne session logs <session-id> --follow` to read without sending
input. Use `ariadne session send <session-id> "Please explain the failure"`
when a script or a one-line response is enough. When a session is waiting on a
permission request, the console lists numbered choices; enter that number (or
the option id or name) to answer it. See [Permission modes](permissions.md).

The desktop app has the same console in a session's detail view: read the
events, type a reply, and select a shown permission option. The agent receives
the same input whichever console you use.

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
