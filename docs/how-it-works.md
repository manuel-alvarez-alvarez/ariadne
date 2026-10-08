# How Ariadne works

Ariadne turns a goal into reviewed work without taking over your checkout. The
daemon drives an orchestrator, task authors, and reviewers as ACP sessions;
each task author works in its own git worktree.

## From goal to change

1. Register a repository and create a goal with a discovered model, and
   optionally a landing. A model is written `<agent-id>:<model-id>`; `ariadne
   models ls` provides the valid names. A goal with no landing of its own
   takes the default landing of its first repository.
2. Connect to the orchestrator with `ariadne goal attach <goal-id>`. It asks
   about anything the goal leaves open. Your answers are console input, not
   commands for an underlying agent CLI.
3. After you agree the plan, the daemon starts the task authors and reviewers
   from their ACP registry commands. It sends prompts, receives events, and
   keeps each conversation alive between turns.
4. Each author owns its task through implementation, review, and landing. Each
   task has a reviewer unless nothing can be tested whole, such as a release
   or a report. A reviewer approves or requests changes. For multiple authors,
   reviewers choose the result to land.
5. The goal's landing decides how its tasks reach the base branch. `none`
   lands nothing. `merge` fast-forwards a squash of each task onto the base
   branch. `pull_request` rebases, pushes the branch, and opens a request
   through the daemon's own call to the forge — never a command the author
   runs itself — and the task ends there: the request's comments, checks and
   merge are not its to wait on. `feature_branch` branches every task from a
   goal branch, one per repository, and merges it there; the one task that
   depends on every other task of that repository then opens the pull
   request from the goal branch to the base branch the same way, and deletes
   the goal branch once that request merges. The daemon removes completed
   worktrees according to its configuration.
6. A request's task ends the moment the daemon opened it, and the pull
   request takes over from there: a repository with the forge integration
   enabled starts a `pr-babysit` session on every open request of yours,
   which answers every comment and clears every failed check until a human
   merges it, and a `pr-reviewer` session on every one asking for your
   review. Neither approves or merges anything. Publishing a request is not
   the same as it being ready to merge: `ariadne attention` carries it as
   "ready to merge" only once every required approval and check reads green,
   and a later change or a failed check takes that back down until the
   session reports ready again. See [The forge integration](forge.md).

## Sessions and attention

A session is the conversation an ACP agent holds for an orchestrator, author,
or reviewer, or one you started directly with an ACP agent outside Ariadne.
`ariadne session ls` lists every one of them, newest activity first, and
`ariadne attention` shows those waiting for a person. Connect with `ariadne
attach <id>` to read events and send a prompt: in a terminal it is an inline
pane with the transcript above and a status line and input box pinned under
it. Escape cancels the running turn, and Ctrl-C twice disconnects your
console without stopping the agent.

`attach` also resumes a stored outside conversation and revives an ended
Ariadne session, each with no separate step first. A resumed outside session
has no goal, no task and no worktree: the agent keeps working in the
directory the original conversation already used. [Resuming a
session](resuming-sessions.md) has the commands.

Permission requests can pause a session. In the CLI or desktop console, select
one of the displayed options. Each repository says whether Ariadne approves,
asks, or learns approvals; see [Permission modes](permissions.md).

## Task lifecycle

Tasks move from `pending` to `ready`, then `in_progress` and `under_review`.
They may return to `in_progress` for requested changes, then become `approved`
and `finished`. They can also be `cancelled` or `failed`; a failed task can be
retried. `ariadne task inspect <task-id>` shows the task, assigned agents,
review history, and ending details.

An author can report that a task cannot be completed as written. Ariadne keeps
the reason with the task so the orchestrator and user can decide whether to
retry, change the task, or cancel it.
