# Adopting a session

Continue a conversation you started directly with an ACP agent by adopting it
as the author of a ready Ariadne task. Ariadne asks the agent for its stored
sessions; it does not scan local transcripts or import a copy of the chat.

## Find and adopt a session

First create or identify a ready task whose author is pinned to the same ACP
agent as the conversation. Then list the agent's stored sessions and adopt the
one you want.

```sh
ariadne session discover
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
