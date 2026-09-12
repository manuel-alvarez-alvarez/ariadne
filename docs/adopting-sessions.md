# Adopting a session

Continue a conversation you started directly with an ACP agent: Ariadne adopts
it as the author of a new task, in a new goal or in a goal already under way.
Ariadne asks the agent for its stored sessions; it does not scan local
transcripts or import a copy of the chat.

## Find the session

Narrow the agent's stored sessions until the one you want is in the table.

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

`session discover` prints the stored session id, agent id, working directory,
last activity, and first prompt. Copy its `id` into `session adopt`.

## Adopt it into a new goal

One call creates the goal, creates and staffs the task, and hands the
conversation to the author:

```sh
ariadne session adopt <session-id> --agent codex-acp \
  --new-goal "Finish the rate limiter" --repo ~/projects/api \
  --author coding,testing=codex-acp:gpt-5.6-sol@xhigh \
  --reviewer code-review=claude-agent-acp:claude-opus-5@high
```

It prints where each of the three got to:

```text
goal 01k71m3cg7f9q6q0f8w4yqz9t1 is now active
task 01k71m3cg8h2r0c2n9a5v2x8k4 is now in_progress
session 01k71m3ch0a4t7m1p3s6y8d2b9 is now running
```

A session that has not finished its handshake yet still reads `starting`. With
`-q` the command prints the task id alone, and `--format json` prints the goal,
the task and the session whole.

The new goal opens active and without an orchestrator: nobody plans it, and the
adopted author works the one task in it. Complete or cancel the goal yourself
when the work is done.

## Adopt it into an active goal

Name a goal that is already running instead, and the task joins it:

```sh
ariadne session adopt <session-id> --agent codex-acp --goal <goal-id> \
  --author coding=codex-acp:gpt-5.6-sol
```

The rest of the line is the same. Use `--repo` to say which of that goal's
repositories the task works in.

## What each flag defaults to

`--agent` and `--author` are always needed, and exactly one of `--goal` and
`--new-goal`. Everything else has a default:

| Flag | Default |
| --- | --- |
| `--title` | the session's first prompt, cut at 120 characters |
| `-d`, `--description` | empty: the title is the whole brief |
| `--repo` | the registered repository that holds the session's working directory |
| `--goal-description` | empty; it belongs to `--new-goal` and is refused with `--goal` |
| `--reviewer` | no reviewer, the same as `--no-reviewer` |
| `--landing` | the way the repository takes a change |
| `--permission-mode` | the configured daemon default ([Permission modes](permissions.md)) |

`--repo` names a registered repository by id or by the path it was added with,
and it is repeatable. A new goal is opened on every repository named; where the
line names several, the task works in the one holding the session's working
directory.

`--author` and `--reviewer` are spelled as `task create` spells them:
`SKILLS=MODEL`, and `@EFFORT` after the model to say how deeply it reasons
there. The reviewers review in the order they are typed.

Continue the conversation with the task id the adoption printed:

```sh
ariadne attach <task-id>
```

## Refusals

Ariadne refuses the adoption when:

- `--goal` names a goal that is not active. Planning, completed and cancelled
  goals take no adopted task.
- No registered repository holds the session's working directory, and no
  `--repo` names one. Add the checkout with `ariadne repo add` first.
- `--author` pins an agent other than the one in `--agent`. The conversation
  can only continue in the agent that stored it.
- The session is no longer listed: already adopted, or gone from the agent.
  Ariadne asks the agents once more before it refuses.
- `--repo` names a repository the goal does not hold.

An adopted session can be adopted once. The agent must advertise ACP session
listing; an agent without it contributes no sessions, and `ariadne session
discover` prints why below the table. To continue an adopted conversation after
a daemon restart, the agent must also support loading a stored session. Check
`ariadne doctor` for each agent's available and unavailable capabilities.
