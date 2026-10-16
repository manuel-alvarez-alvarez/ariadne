# Following what happens

Use this page after an orchestrator starts a goal. Start with attention, then
watch the goal's tasks or event stream. Nothing here polls: the daemon streams,
and every follow mode reads that stream. Ctrl-C ends any of them.

## Watch a goal

```sh
ariadne attention --watch              # work that needs you, across every goal
ariadne task ls --watch --goal <goal-id>
ariadne events --follow --goal <goal-id>
```

`ariadne attention` identifies work that needs a person. Follow its printed
action. Attach only a goal, task, or session. Attention items can also name
settings or pull requests. `task ls` shows each task's current workflow column.
`events` shows the latest 200 recorded events, oldest first, then continues
from that point. Its `--goal`, `--task`, `--session`, and `--kind` filters
narrow both the opening list and the stream.

## Follow one agent

```sh
ariadne session logs <session-id> --follow
ariadne task logs <task-id> --follow
ariadne session logs <session-id> --tail 20
ariadne task logs <task-id> --since 10m --kind agent_message
```

Session and task logs print full transcript blocks. Agent messages and
thoughts stream while the agent writes them. `--tail` and `--since` narrow the
opening transcript; repeat `--kind` to select several event kinds.

To reply, use [the console](cli.md#connect-to-a-session). To return to an
ended or outside conversation, use [Resuming a session](resuming-sessions.md).

## Other streams

```sh
ariadne events --follow --format json   # one JSON object per line, for a pipe
ariadne daemon logs --follow            # the daemon log, over the API
ariadne goal ls --watch
ariadne session ls --watch --seat agent
ariadne pr ls --watch
```

`--follow` prints each new item. `--watch` redraws a table when an event
changes it. `ariadne daemon logs` reads the daemon's ring buffer, so it works
with launchd and systemd; it uses the local log file only when the daemon does
not answer.

With `ai_failure_diagnosis` on (see [Configuration](configuration.md)), a
failed session may later carry a `session.diagnosis` event. It is advisory and
does not retry a session or switch its model.

A repository with [the forge integration](forge.md) also sends
`pull_requests_changed` and `forge_settings_updated` events. They have no goal
or task, so `--goal` and `--task` omit them; `--kind` still selects them.
