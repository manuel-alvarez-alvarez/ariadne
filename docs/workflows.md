# Workflows

A workflow is a linear kanban of columns. Every goal runs on one: each task
of the goal is staffed one agent per column, and it walks the columns on one
branch, in one worktree, until the last column's gate lets it finish.

Ariadne ships two workflows, `develop-review-merge` and `develop-review-pr`.
`ariadne workflow` manages the catalog the way `ariadne skill` manages
skills: a shipped workflow is reset rather than deleted, and one of your own
is deleted rather than reset.

## The document

A workflow is a text document. The first line names it, and every other
non-blank line belongs to a column:

```
workflow develop-review-merge
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    rank: balanced
    gate: committed
  review[Review]
    Run the whole suite and judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    rank: frontier
  merge[Merge]
    Rebase onto the base branch, run the whole suite, squash, fast-forward and push.
    skills: merge
    rank: fast
    gate: merged
```

1. The first non-blank line is `workflow <name>`. The name is kebab-case.
2. A column line is `<id>[<Title>]`. The id is kebab-case, and unique in one
   document.
3. Every non-blank line after a column line, and before the next one,
   belongs to that column.
4. A body line names a key or adds to the column's description:
   - `skills: a, b` names the column's skills, comma-separated.
   - `rank: <word>` names the column's preferred rank: `fast`, `balanced`,
     `frontier` or `local` (see [the four ranks](cli.md#work-with-tasks)).
   - `gate: <word>` names what the column checks before a task moves on:
     `committed`, `pushed`, `merged` or `request-merged`.
   Each key appears at most once per column. Any other line is a
   description line, and consecutive description lines join with one
   space. Skills, rank, gate and description are all optional.
5. Blank lines and indentation are ignored. At least one column is
   required.
6. A document that breaks one of these rules is refused, naming the line
   it broke on and what went wrong.

## The two shipped workflows

`develop-review-merge` — the document shown above — lands a task on the
base branch itself: its `merge` column rebases the task, runs the suite,
squashes, fast-forwards and pushes.

`develop-review-pr` shares its first two columns, and ends at a pull
request instead: its `pr` column pushes the branch, opens the request, and
keeps it until a human merges or closes it.

```
workflow develop-review-pr
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    rank: balanced
    gate: committed
  review[Review]
    Run the whole suite and judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    rank: frontier
  pr[Pull request]
    Push the branch, open the request and keep it until a human merges or closes it.
    skills: pr-babysit
    rank: balanced
    gate: request-merged
```

`develop-review-merge` is the default workflow of a repository that names
none.

## Gates

A column's gate is what `complete_step` checks before the task moves to
the next column, or finishes from the last one:

| Gate | What it checks |
| --- | --- |
| none | Nothing; the step moves on the reason alone. |
| `committed` | At least one commit past the base branch, with a clean worktree. |
| `pushed` | The remote holds the branch's current tip. |
| `merged` | The supplied merge commit, and the task's branch, are both ancestors of the base branch. |
| `request-merged` | A fresh read of the forge says the task's pull request merged. |

A gate that fails answers 409 and leaves the task on its column; nothing
moves. The current column's agent can open the task's pull request before
completing a column gated on `request-merged`. See [The forge
integration](forge.md) for what the `pr` column's agent does with the
request it keeps.

## Rank

A column's `rank` says how capable a model its agent should run on by
default: `fast` is quick and cheap, `balanced` is an everyday model,
`frontier` is the most capable one, and `local` runs on your own machine.
`ariadne models rank` sets which model answers to each rank — see [Using
the CLI](cli.md#work-with-tasks). A column with no rank is staffed on
whichever model the task's creator or the orchestrator picks.

## The catalog

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
none is named or where `-` names it. `check` parses a draft without saving
it anywhere, and prints its columns, or the line a rule broke on. `show`
prints the document whole. Resetting puts a shipped workflow back on its
shipped text; deleting removes one of your own. Neither verb works on the
other kind: a shipped workflow refuses delete, and one of your own refuses
reset. A workflow a goal or a repository still names refuses delete too.

In the desktop app, the Workflows screen lists shipped and your own
workflows in two groups, each opening a document editor with a parsed
kanban preview beside it.

## The default and the override

A repository has a default workflow, `develop-review-merge` until you set
another:

```sh
ariadne repo add ~/projects/api --workflow develop-review-pr
ariadne repo update <repo-id> --workflow develop-review-merge
```

In the desktop app, the repository dialog has a default workflow picker.

A goal takes a workflow of its own, or its first repository's default
where it names none:

```sh
ariadne goal create --title "Add rate limiting" --repo ~/projects/api \
    --model codex-acp:<model-id> --workflow develop-review-pr
```

In the desktop app, the goal dialog's workflow picker starts at the first
picked repository's default workflow; picking another replaces it. A
goal's workflow is fixed once the goal is created, and a repository's later
change to its own default changes no goal already running.

Creating the goal copies the workflow's columns into it, so a later edit to
the catalog affects only goals created afterwards.

## Staffing a task

A task is staffed one agent per column, with `--agent
STEP[:SKILLS]=MODEL[@EFFORT]`, repeatable:

```sh
ariadne task create <goal-id> --title "Add the rate limiter" \
    --agent develop:coding=codex-acp:<model-id> \
    --agent review:code-review=claude-acp:<model-id>@high \
    --agent merge=codex-acp:<model-id>
```

Leaving `:SKILLS` out stages the column's own skills. While a goal is still
`planning`, a task may leave a column unstaffed, so the orchestrator can
staff it in a later call; finalizing the plan refuses a task with a column
nobody staffs, naming the task and the column. Once the goal is active,
every create or edit has to staff every column, or it is refused by the
column's name. `task update --agent` replaces the whole staffing: an agent
already on its column keeps its row, its sessions and its usage; a column
staffed for the first time gets a new row; a column the edit leaves out
loses its agent only while nothing names it.

## Moving between columns

All of a task's columns share its branch and worktree. The task starts on
the first column and stays `in_progress` until it finishes or fails; which
column it is on is its `step`, not a status of its own. Only the current
column's agent can move it:

- `complete_step`, with a reason and an optional `merge_commit`, checks the
  column's gate, then moves the task to the next column, or finishes it
  from the last one.
- `fail_step`, with a reason, moves the task back one column for changes.
  Called from the first column, there is no column before it to move back
  to, so it fails the task instead.

Every move records both columns and its reason as one transition. Entering
a column for the first time briefs its agent with the task, the column's
description and skills, the base branch, and the previous column's summary.
A later entry tells the agent whether the task came back for changes, came
forward again, or is a retry, and why.

Any column's agent can also call `fail_task` directly, which fails the task
outright rather than moving it back a column — the way to say a task
cannot be done as written, whatever column it is on. A retry puts a failed
task back on its first column, reusing that column's agent's conversation
where one is still open on the pin it ran on.

## Sources

[How Ariadne works](how-it-works.md) follows one task through its columns
from `pending` to `finished`.
