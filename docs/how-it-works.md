# How Ariadne works

Ariadne turns a goal into reviewed work without taking over your checkout. The
daemon drives an orchestrator, task authors, and reviewers as ACP sessions;
each task author works in its own git worktree.

## From goal to change

1. Register a repository and create a goal with a discovered model. A model is
   written `<agent-id>:<model-id>`; `ariadne models ls` provides the valid
   names.
2. Connect to the orchestrator with `ariadne goal attach <goal-id>`. It asks
   about anything the goal leaves open. Your answers are console input, not
   commands for an underlying agent CLI.
3. After you agree the plan, the daemon starts the task authors and reviewers
   from their ACP registry commands. It sends prompts, receives events, and
   keeps each conversation alive between turns.
4. Each author owns its task through implementation, review, and landing. A
   reviewer approves or requests changes. For multiple authors, reviewers
   choose the result to land.
5. The configured landing mode either fast-forwards a squash merge, opens and
   completes a pull request, or records work that does not land code. The
   daemon removes completed worktrees according to its configuration.

## Sessions and attention

A session is the conversation an ACP agent holds for an orchestrator, author,
or reviewer. `ariadne session ls` shows live sessions and `ariadne attention`
shows those waiting for a person. Connect with `ariadne attach <id>` to read
events and send a prompt; Ctrl-C disconnects your console but does not stop
the agent. `ariadne session resume <session-id>` starts a new agent process
for an ended conversation when its agent can restore it.

Permission requests can pause a session. In the CLI or desktop console, select
one of the displayed options. Configure whether Ariadne approves, asks, or
learns approvals in [Permission modes](permissions.md).

## Task lifecycle

Tasks move from `pending` to `ready`, then `in_progress` and `under_review`.
They may return to `in_progress` for requested changes, then become `approved`
and `finished`. They can also be `cancelled` or `failed`; a failed task can be
retried. `ariadne task inspect <task-id>` shows the task, assigned agents,
review history, and ending details.

An author can report that a task cannot be completed as written. Ariadne keeps
the reason with the task so the orchestrator and user can decide whether to
retry, change the task, or cancel it.

## Continue an existing conversation

An ACP agent can list conversations that it stored outside Ariadne. Use
`ariadne session discover` to find them, then `ariadne session adopt` to create
a task for one — in a new goal or in a goal already under way — with that
conversation as its author. [Adopting a session](adopting-sessions.md) has the
compatibility rules and commands.
