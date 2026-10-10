# How Ariadne works

Ariadne turns a goal into finished work without taking over your checkout.
The daemon drives an orchestrator and, for every task, one agent per column
of the goal's workflow, each as an ACP session; a task's columns all work in
its own git worktree.

## From goal to change

1. Register a repository and create a goal with a discovered model, and
   optionally a workflow. A model is written `<agent-id>:<model-id>`;
   `ariadne models ls` provides the valid names. A goal with no workflow of
   its own takes the default workflow of its first repository.
2. Connect to the orchestrator with `ariadne goal attach <goal-id>`. It asks
   about anything the goal leaves open, then writes the tasks and staffs
   each one's columns. Your answers are console input, not commands for an
   underlying agent CLI.
3. Once you agree the plan, the daemon starts every task's first column. It
   sends prompts, receives events, and keeps each conversation alive between
   turns.
4. A task walks its workflow's columns in order, one agent per column,
   sharing one branch and one worktree throughout. The current column's
   agent calls `complete_step`, with a reason, to move the task to the next
   column or finish it from the last one; it calls `fail_step` to send the
   task back one column for changes, with what to change. A development
   column builds and commits the change; a review column runs the whole
   suite and judges it against the task and the repository's rules, failing
   the step back where it finds a problem. See [Workflows](workflows.md) for
   the two shipped workflows, every column's gate, and how to write your
   own.
5. The last column's gate decides whether the task is actually done. In
   `develop-review-merge`, its `merge` column rebases the task onto the
   base branch, runs the whole suite, squashes, fast-forwards and pushes.
   In `develop-review-pr`, its `pr` column pushes the branch and opens a
   request through the daemon's own call to the forge — never a command
   the agent runs itself — and keeps the request until a human merges or
   closes it.
6. The `pr` column's agent keeps its request open with the `pr-babysit`
   skill until a human merges or closes it; a request asking for your
   review gets a `pr-reviewer` session instead. See [The forge
   integration](forge.md) for how each keeps up with the request, and what
   `ariadne attention` shows while one waits on you.

## Sessions and attention

A session is the conversation an ACP agent holds for the orchestrator, for
one column's agent, or one you started directly with an ACP agent outside
Ariadne. `ariadne session ls` lists every one of them, newest activity
first, and `ariadne attention` shows those waiting for a person. Connect
with `ariadne attach <id>` to read events and send a prompt: in a terminal
it is an inline pane with the transcript above and a status line and input
box pinned under it. Escape cancels the running turn, and Ctrl-C twice
disconnects your console without stopping the agent.

`GET /v1/attention` is the one list both the CLI and Ariadne Desktop read
for a blocker only a person can clear: a model's quota with no automatic
switch left, a task failed on a machine's own descriptor limit, or a forge
CLI missing from the daemon's PATH. Automatic recovery still trying — a
model switch about to run, a forge poll about to retry — raises nothing
there; only recovery that has spent its options does. See [Scheduler,
attention and watchdogs](../specs/009-scheduler-attention-and-watchdogs.md)
for the rules, and [Needs attention](../specs/031-needs-attention.md) for
the item shape and the producers that fill it.

`attach` also resumes a stored outside conversation and revives an ended
Ariadne session, each with no separate step first. A resumed outside session
has no goal, no task and no worktree: the agent keeps working in the
directory the original conversation already used. [Resuming a
session](resuming-sessions.md) has the commands.

Permission requests can pause a session. In the CLI or desktop console,
select one of the displayed options. Each repository says whether Ariadne
approves, asks, or learns approvals; see [Permission modes](permissions.md).

## Task lifecycle

A task is `pending` while its dependencies are still running, then `ready`
once every dependency has finished. It becomes `in_progress` the moment the
first column's agent starts, and stays `in_progress` from there to its last
column — the column it is on is its `step`, not a status of its own — until
the last column's gate lets it finish. It can also be `cancelled` by the
user or the orchestrator, or `failed`, by any column's agent or by the
daemon, and later retried: a retry puts it back on its first column.
`ariadne task inspect <task-id>` shows the task, its workflow, each column's
agent, and the reason it carries, whichever way it ended.

Any column's agent can fail the task with a reason instead of moving it.
Ariadne keeps that reason with the task so the orchestrator and user can
decide whether to retry, change the task, or cancel it.
