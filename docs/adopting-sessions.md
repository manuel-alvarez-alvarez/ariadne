# Adopting a session

Continue a conversation you started directly with an ACP agent by adopting it
as the author of a ready Ariadne task. Ariadne asks the agent for its stored
sessions; it does not scan local transcripts or import a copy of the chat.

## Find and adopt a session

First create or identify a ready task whose author is pinned to the same ACP
agent as the conversation. Then narrow the agent's stored sessions and adopt
the one you want.

```sh
ariadne session discover --agent codex-acp --dir ~/projects/api \
  --since 2026-09-01 --search "rate limit" --limit 1
```

The table ends with its page count and, when another page exists, the complete
command for fetching it:

```text
1 of 3 sessions
Next: ariadne session discover --agent codex-acp --dir /home/me/projects/api --since 2026-09-01T00:00:00Z --search 'rate limit' --limit 1 --cursor <token>
```

Run that `Next` command to fetch one more page, or add `--all` to the first
command to fetch every page into one table. `--all` cannot be combined with
`--cursor`.

Without filters, discovery includes every outside session. `--agent` selects
one registry agent, `--dir` includes that directory and its descendants, and
`--search` matches part of the first prompt without case sensitivity. Use
`--until` to set the other activity bound. Both activity bounds accept an RFC
3339 time or a `YYYY-MM-DD` date. A date means the start of its UTC day for
`--since` and the end for `--until`. Use `--refresh` when you need the daemon
to ask the agents again instead of using its current snapshot. Without
`--limit`, each page contains up to 50 sessions; the maximum is 200.

Then adopt the selected session and attach to it:

```sh
ariadne session adopt <session-id> <task-id> --agent codex-acp
ariadne attach <task-id>
```

`session discover` prints the stored session id, agent id, working directory,
last activity, and first prompt. Copy its `id` into `session adopt`. The command
prints Ariadne's new author-session id; use that id or the task id with
`ariadne attach` to continue the conversation.

## Requirements and limits

The task must be `ready`, and its author must be pinned to the agent named by
`--agent`. A stored session can be adopted only into one task. Ariadne refuses
the adoption if the session is no longer listed, is already managed by
Ariadne, or belongs to another agent.

The agent must advertise ACP session listing. An agent without it contributes
no sessions, and `ariadne session discover` prints why below the table. To
continue an adopted conversation after a daemon restart, the agent must also
support loading a stored session. Check `ariadne doctor` for each agent's
available and unavailable capabilities.
