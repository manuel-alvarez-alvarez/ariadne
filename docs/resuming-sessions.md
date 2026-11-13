# Resuming a session

Continue a session of a goal, task or PR review with `ariadne attach`.

## Find the session

```sh
ariadne session ls
ariadne session ls --all --task <task-id>
ariadne session ls --agent codex-acp --dir /absolute/project/path --search "rate limit"
```

The table lists orchestrator, column-agent, reviewer and keeper sessions, newest activity first.
Its columns include `id`, `title`, `status`, `goal`, `task`, `seat`, `agent`, `model`, `age` and `tokens`.
Add `directory` with `--columns`.
The title comes from the goal, task or PR review.

The default window covers seven days of live sessions.
Use `--all` to include ended sessions and fetch every page.
Use `--since` or `--until` to set activity bounds.
Each accepts RFC 3339 or `YYYY-MM-DD`; dates use UTC day boundaries.
Use `--goal`, `--task`, `--status`, `--seat` or `--attention` to narrow the results.
`--dir` matches a directory and its descendants; `--search` matches title text without case sensitivity.
`--limit` defaults to 50, with a maximum of 200.
Use the printed next-page command to continue with `--cursor`.

## Attach and go

```sh
ariadne attach <session-id>
ariadne attach <task-id>
ariadne attach <goal-id>
```

An ended session resumes in a new agent process with the same stored conversation.
It keeps its goal, task and seat.
If its worktree is gone, the agent uses the repository checkout.
Reviving a session does not restart a completed goal or change its workflow.

The [CLI console](cli.md#connect-to-a-session) shows the transcript and accepts the next prompt.
The agent keeps working after you disconnect.

## Switching to another agent

Change the agent or model a session uses without stopping the conversation:

```sh
ariadne session switch <id> --model codex-acp:<model-id>
ariadne session switch <id> --model claude-acp:<model-id> --effort high
```

Ariadne Desktop shows a Switch button on the session screen.

A switch to another agent or a different model ends the conversation and
starts a new session on the same seat. The new session reads the old
one's prompts, the agent's messages, and the tool calls, folded up, under a
240,000-character cap. When the complete folded history fits, it arrives
unchanged. When it does not, Ariadne keeps whole entries with deterministic
rules: a real user correction, then a real user prompt, an unfinished plan,
a blocker or failure, an agent decision, a file change, and a useful completed
test result. Later entries break a tie, and the selected entries return in
their original order.

Only the stored transcript role and the visible folded entry provide evidence.
Text printed by a tool cannot become a user instruction, and daemon prompts
do not receive user priority. A prompt that cannot fit as one whole entry is
left out. If several user prompts compete, the later correction or prompt is
kept. When no entry supplies usable evidence, Ariadne keeps the newest entries
as before. Omission notes count against the cap and state when omitted entries
come from more than one gap. These rules use no model inference or generated
summary. They came from constructed pilot examples; they do not establish
performance on real session histories.

The seat's pin moves with the new session, so later spawns and resumes reach
the new one.

A switch to the same agent keeps the conversation: the agent takes the new
model in place.

## When it refuses

An agent must support ACP resume or load to reopen its stored conversation.
Run `ariadne doctor` to inspect its capabilities.
A session without an internal agent id cannot resume.
A session of a cancelled goal cannot resume.
