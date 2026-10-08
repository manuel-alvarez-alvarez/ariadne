# Following what happens

Nothing here polls: the daemon streams, and every follow mode below reads that
stream. Ctrl-C ends any of them and leaves the terminal as it found it.

```sh
ariadne events                         # the 200 most recent events, oldest first
ariadne events -f                      # ...and keep printing as it happens
ariadne events -f --goal <goal-id>     # one goal's; also --task, --session, --kind
ariadne events -f --format json        # one JSON object per line, for a pipe

ariadne session logs <session-id> -f   # an agent's event transcript, until it ends
ariadne task logs <task-id> -f         # the same, found by task (--seat reviewer)
ariadne session logs <id> --tail 20    # only the last twenty transcript blocks
ariadne task logs <id> --since 10m --kind agent_message
ariadne daemon logs -f                 # the daemon's own log, over the API

ariadne attention --watch              # redrawn whenever something needs you
ariadne task ls --watch --goal <id>    # redrawn whenever a task moves
ariadne goal ls --watch                # redrawn whenever a goal moves
ariadne session ls --watch --seat reviewer  # redrawn whenever a session moves
```

`ariadne events` opens on the most recent recorded events — the last 200 of
them, printed oldest first — so `-f` goes on from where that snapshot ends.
Every filter narrows the snapshot too, `--goal` included.

Session and task logs print full transcript blocks. Agent message and thought
chunks stream as the agent writes them, under one header per item. `--tail`
and `--since` narrow the opening snapshot, which is the session's 200 newest
events. Repeat `--kind` to select several event kinds; it also filters new
events during `-f`. JSON mode keeps each event object unchanged.

`-f` prints as it goes; `--watch` redraws the whole table, `watch(1)`-style,
when an event says it has changed. `ariadne daemon logs` reads the daemon's own
ring buffer, so it works under launchd and systemd — where nothing writes
`~/.ariadne/ariadned.log` — and honours `--endpoint`; the file is the fallback
for a daemon that is not answering.

With `ai_failure_diagnosis` on (see [Configuration](configuration.md)), a
failed session may later carry a `session.diagnosis` event too: an advisory
reading of the error beside it — quota exhaustion, a temporary failure, an
authentication or configuration problem, a task failure, or insufficient
evidence — from the local model `ai` permission mode already uses (022). It
appears in `ariadne events`, in `ariadne session logs` and `ariadne task
logs` as a line under the `ERROR` block it is about, and in the desktop
app's session activity, however much later it arrives and however the
session has ended by then. It never changes what the error itself says, and
never decides whether a session retries or switches models: the model may
disagree with the daemon's own exhaustion check, and that disagreement is
never acted on (021, 024).

A repository with the forge integration on (see [The forge
integration](forge.md)) also carries `pull_requests_changed` in `ariadne
events -f`, with the repository's id as the subject, whenever its requests
move on the forge or in what Ariadne keeps of them — the requests
themselves are read live, so the event carries none; and
`forge_settings_updated`, subject "forge tunnel", whenever the webhook
tunnel's switch or state changes. Neither belongs to a goal or a task, so
`--goal` and `--task` leave them out; `--kind` still selects them, and
`ariadne pr inspect <repo> <number>` reads a request's current state
directly.
