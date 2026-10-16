# Resuming a session

Use this page when you return to a conversation. While its goal runs, use
[Following what happens](following-events.md) to watch its tasks and
attention.

## Find the session

`ariadne session ls` lists Ariadne sessions — for an orchestrator, task
column, or reviewer — with outside conversations that an ACP agent holds but
Ariadne did not start. The table is newest first and shows each session's id,
title, status, goal, task, agent, model, age, and tokens.

```sh
ariadne session ls --goal <goal-id> --all
ariadne session ls --attention
ariadne session ls --kind outside --agent codex-acp --dir ~/projects/api \
  --since 2026-09-01 --search "rate limit"
```

The default list covers the previous seven days. `--all` includes ended
Ariadne sessions and every page. Filter by `--goal`, `--task`, `--seat`, or
`--attention` to return to work from a goal. Filter by `--kind`, `--agent`,
`--dir`, or `--search` to find a conversation outside Ariadne. An outside row
has no status, goal, or task.

## Continue the conversation

Use `attach` to open a console. It accepts a session, task, or goal id. A task
selects its current column's agent and a goal selects its orchestrator.

```sh
ariadne attach <session-id>                  # live or ended Ariadne session
ariadne session resume <session-id>          # revive it without opening a console
ariadne attach <task-id>                     # the task's current column
ariadne attach <goal-id>                     # the goal's orchestrator
ariadne attach <outside-session-id> --agent codex-acp
```

Attaching an ended Ariadne session revives its agent before the console opens.
Attaching an outside id creates a loose Ariadne session and opens that
conversation.
Use `--agent` only when two agents hold the same outside id.

The console sends each line as the next prompt. The agent continues after you
leave. See [Using the CLI](cli.md#connect-to-a-session) for console controls.

## Know the session type

An Ariadne session keeps its goal, task, and workflow column. It uses its
worktree while the daemon keeps that worktree. Reviving replaces only its agent
process; it does not change the task plan or review.

Attaching an outside conversation creates a loose Ariadne session. It has no
goal, task, workflow column, or managed worktree. It works in its original
directory against the checkout as it stands. Ariadne does not merge its
changes or open a pull request for it.

## Switch the model or agent

```sh
ariadne session switch <session-id> --model codex-acp:<model-id>
ariadne session switch <session-id> --model claude-acp:<model-id> --effort high
```

Switching agents or models starts a new session on the same seat and carries
the previous conversation as folded context. Switching to another model of the
same agent keeps the conversation. The seat then uses the new session for later
spawns and resumes.

## When it does not resume

Run `ariadne doctor` when the session is absent or cannot resume. The report
names agents that lack session listing or loading support.
