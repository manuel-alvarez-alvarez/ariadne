# How Ariadne works

A goal becomes tasks, a task gets its authors, and a review gates its landing.
This page follows one goal from the moment you describe it to the moment the
daemon has nothing left to run.

## From a goal to a landed change

1. `ariadne goal create` — you describe a goal, pick the registered
   repositories it works in (`ariadne repo add`). The daemon spawns the
   **orchestrator** in tmux; `ariadne goal attach` drops you into its terminal.
   `--model` is required, and it is the whole choice: a model is spelled
   `<agent_kind>:<model>` — the agent CLI that runs it (`acp`, `claude_code`,
   `codex`, `opencode`) and, after a colon, one model of that CLI (`--model
   codex:gpt-5.6-sol`). A bare CLI name or `default` is refused because every
   run must name its model. `--effort` goes beside it and
   says how deeply that model reasons — one of the efforts `ariadne models ls`
   lists for it (`--effort xhigh`); left out, the model runs at whatever its
   agent CLI runs it at.
2. The orchestrator asks its questions in its terminal and waits — `ariadne
   goal attach` drops you into that terminal to answer them, and an
   orchestrator waiting on you shows up wherever Ariadne lists what needs
   attention. It asks until nothing about the goal is open. It never writes
   code, and it writes no specification of its own: where a goal wants one,
   that is a task like any other, staffed with the `spec-writing` skill.
3. Then it splits the goal into tasks through the Ariadne MCP tools, with
   optional `depends_on` ordering, which is for real dependencies alone. Each
   task is staffed with its **authors** — one for most tasks — and the agents
   that review it. A hard task can staff several authors, each on its own
   model and a branch of its own (`--author`, repeated): each writes the task
   alone, each attempt is reviewed to approval, and the reviewers then pick
   the one change that lands — the branches they pass over are removed with
   their worktrees. A task with several authors needs at least one reviewer,
   to pick. What an
   agent can do is the **skills** it loads — one document about one kind of
   work, from the catalog Ariadne ships and the ones you write (`ariadne skill
   ls`) — so an agent has no identity beyond its skills, its model and its
   brief.
4. Three things the orchestrator settles with you rather than deciding alone:
   what the goal asks for, which tasks are worth a review, and how each task
   ends. That last one is the task's `landing`: `merge` puts the change on the
   base branch, `pull_request` opens a request and sees it through, and `none`
   lands nothing at all — a released tag, a filed report, a document that lives
   elsewhere. A task with nothing to review is staffed with no reviewer and is
   approved as soon as its author asks.
5. Nothing runs while the goal is in `planning` — read the tasks and edit what
   they still need with `ariadne task update` — and it is `finalize_plan` that
   starts the work. The orchestrator also picks what each agent runs on,
   sizing the model and the effort to the task it wrote; the last word is
   yours, with `ariadne task update <task-id> --model claude_code:claude-opus-5
   --effort xhigh --reviewer code-review=codex:gpt-5.6-luna@high`, which a task
   takes while it is still pending or ready. A reviewer is spelled
   `SKILLS=MODEL[@EFFORT]` — `code-review=codex:gpt-5.6-luna` without a chosen
   effort, or `code-review=codex:gpt-5.6-luna@high` to reason harder — and
   `ariadne task create` takes the same flags, plus
   `--no-reviewer` and `--landing`. `list_models` describes what the
   orchestrator sizes a task from: each model's tier, a cost and a speed band,
   what task shapes it is and is not a fit for, and what each of its efforts
   buys — `ariadne models show <model>` prints the same card.
6. The scheduler takes over: when a task's dependencies are finished it becomes
   `ready` and each of its **authors** is spawned in a dedicated git worktree,
   on a branch named after the task — its title slugged plus a short tail of
   its id, as in `fix-the-landing-briefing-real-fetch-r9jr7c`; a second author
   works beside the first as `…-r9jr7c-a2`. Each implements,
   commits and calls `request_review` under a summary of what it did, which is
   what the reviewers read first.
7. **Reviewers** spawn in read-only detached worktrees, inspect the diff and
   `submit_verdict`, approving or requesting changes. Change requests resume
   the author with the feedback; every reviewer approving moves the task to
   `approved`. On a task with several authors each attempt is reviewed on its
   own, the task moves on only once every author is approved and every
   reviewer has called `pick_winner`, and the most-picked author's branch is
   the one that lands — `ariadne task inspect` shows the authors and the
   picks.

   The agents write to each other while that happens: `send_message` asks an
   agent of the task, or the orchestrator, what it needs to know, and answers
   what it is asked. That is all it carries — no acknowledgements, no thanks,
   nothing about what an agent is about to do — because every message lands in
   the recipient's pane as a turn, and a courtesy costs a turn. There is no
   inbox to check either: Ariadne types the message in and submits it. A
   verdict is a message of the kind that settles a review, and it all reads
   back as one
   channel — `ariadne task messages <task-id>`, or the Messages tab of the
   task panel. Every agent of a task stays up until the task is over, so a
   reviewer that has voted is still there to be asked something.
8. The task never leaves the author that wrote it: it keeps its session and
   its worktree, and is briefed with the procedure that ends the task — the
   whole thing, which the author then runs. There is one procedure per ending,
   and it is Ariadne's own: a repository is a checkout and a base branch, and
   says nothing about how work ends in it. What the three are:
   - **`merge`** — rebase onto the base, squash into one commit with a
     conventional subject, fast-forward the base branch in the primary
     checkout, push it where there is a remote, then `finish_task`. The daemon
     only accepts the sha after verifying the merge with
     `git merge-base --is-ancestor`.
   - **`pull_request`** — rebase once, push the branch, and open a request with
     `gh pr create` or `glab mr create` (whichever the `origin` remote calls
     for), following the repository's own templates; `record_pull_request`
     tells you where it is. The author then waits on it in its own session,
     polling the forge and sleeping between polls: it answers every comment,
     and a change somebody asks for is made on the same branch and sent through
     the Ariadne reviewers before it is pushed — a published branch is merged
     into and added to, never rewritten. Once the request is approved and green
     it merges it with `--squash`, fast-forwards the base branch and reports
     the sha.
   - **`none`** — nothing is landed. The author checks that what the task asked
     for is where the task said to put it, and that nothing is left only in
     the worktree, which is thrown away with the task.

9. Worktrees are cleaned up and dependent tasks wake up. The orchestrator is
   still there — it stays up for the whole goal, which is why you can attach
   to it at any point and ask what is going on — and the daemon tells it when
   a task fails, when one goes quiet, and when there is nothing left to do. It
   answers with `retry_task`, `cancel_task`, `update_task`, or `complete_goal`
   once the goal is met. `ariadne goal complete` is the same call from the
   terminal.

## The task lifecycle

Task lifecycle: `pending → ready → in_progress → under_review →
(changes_requested → in_progress …) → approved → finished`, with
`cancelled`/`failed` (retryable) escapes. From `approved` the author can also
`request_review` again, which is how a revision of a published request is
reviewed. A task that cannot be done as written is the author's own
`fail_task`, whose reason is what `ariadne task inspect` shows you. Every
transition is validated against a typed state machine and recorded in an audit
table.

## The English the agents read and write

Every text Ariadne hands an agent is written in ASD-STE100 **Simplified
Technical English**: the three system prompts, the briefings that start and
resume a session, and the description of every MCP tool. One instruction to a
sentence, the imperative for an instruction, the active voice, short sentences
(20 words in a procedure, 25 in a description), one meaning per word, a list
for a sequence of steps — what an agent misreads least, in the fewest tokens.
The agents write it too: the session rules every agent receives before its
first prompt, whatever its seat and whatever skills it loaded, hold
everything it writes to the same English — its turn text and visible
reasoning, task titles and descriptions, review summaries, verdicts, failure
reasons, commit subjects and bodies, and pull request text.

## Permissions and hooks

Agents run with permissions bypassed — `--dangerously-skip-permissions` for
Claude Code, `--dangerously-bypass-approvals-and-sandbox` for Codex, `--auto`
plus an allow-everything permission block for OpenCode (`ariadne agent ls`
prints the current flags). ACP has no common bypass flag, so its default list
is empty. Add the selected agent's flag with `ariadne agent update acp`.

Hooks installed at spawn time report every session and tool event to the
daemon. The ACP client maps protocol updates into the same event vocabulary.
Each internal session id is tracked, so sessions can be resumed and attached.

## Sessions and compaction

Sessions are long-lived — one session per author of a task, one reviewer per
task across every review of it, one orchestrator per goal — and every resume replays the whole
transcript as its first prompt. Shortening that transcript is the agent's own
business: compact a session by typing `/compact` in its pane, or leave it to
the CLI near its context limit. The daemon asks for none, and types into a
pane only what the work gives it — a nudge, a review's feedback, a landing
briefing, a message from another agent. It reads a compaction the CLI reports
(Claude Code's `SessionStart` from `compact`, Codex's `PostCompact` hook,
OpenCode's `session.compacted` event, or ACP's completed `compaction_update`)
for what it says about the agent. The turn is over, and the agent is ready.
