# Resuming a session

Continue a conversation instead of starting one: an Ariadne session that has
ended, or one you started directly with an ACP agent, outside Ariadne
entirely. `ariadne session ls` lists both kinds in one table, and `ariadne
attach` opens either one.

## Find the session

```sh
ariadne session ls
```

lists Ariadne's own sessions — an orchestrator's, an author's, a reviewer's —
next to outside sessions: conversations an ACP agent holds that Ariadne never
started. Rows come newest activity first. The columns are `id`, `title`,
`status`, `goal`, `task`, `agent`, `age` and `tokens`; add `directory` with
`--columns`. An outside session has no status, goal or task of its own yet,
so those columns are empty for it.

An outside conversation comes from two places at once. Ariadne asks every
agent for its own list over ACP, and it reads the files the agent keeps —
for Claude Code, the transcripts under `$CLAUDE_CONFIG_DIR`, or `~/.claude`
where that is unset. Each agent answers its list from an index of its own
that leaves conversations out: on one machine Claude's index held 731 of the
740 transcripts beside it. A conversation both places hold is one row, under
the title the agent gave it. A transcript with neither a prompt nor an answer
in it is no conversation of its own, so it reaches the table only when the
agent's own list holds it — nothing the files say takes a row of that list
away. Ariadne reads an agent's files only while that agent is installed and
working, because a conversation it cannot reopen is one you cannot attach to.

The files are also where an outside row's `tokens` and the model it ran come
from: an agent's list carries neither. Ariadne reads them for the rows of the
page it answers, so `--limit` bounds the reading as well as the table.

Narrow the table to the session you want:

```sh
ariadne session ls --kind outside --agent codex-acp --dir ~/projects/api \
  --since 2026-09-01 --search "rate limit"
```

`--kind` limits the table to Ariadne's own sessions or to outside ones,
`--agent` to one registry agent, `--status` to a session status, `--seat` to
`orchestrator`, `author` or `reviewer`, and `--goal` or `--task` to one
goal's or task's sessions. `--attention` keeps only sessions waiting for a
person. `--dir` matches a working directory and everything under it, and
`--search` matches part of the title or first prompt, without case
sensitivity. `--since` and `--until` bound activity, each an RFC 3339 time or
a `YYYY-MM-DD` date — the start of its UTC day for `--since`, the end for
`--until`; without either, the table holds only the last 7 days. `--limit`
and `--cursor` page the table, `--all` fetches every page into one instead,
and `--refresh` asks the agents again instead of using the daemon's current
snapshot.

## In Ariadne Desktop

The Sessions screen lists the same two kinds together, in one table, with a
window control over it: the last 7 days it opens on, the last 30, or all of
it. An outside row's Agent column names the pin it last ran on, and its
Tokens column what it has spent, with the detail behind a tooltip, once the
daemon reports them; until then the Agent column keeps naming the registry
agent it belongs to, and the Tokens column stays blank rather than showing a
zero.

## Attach and go

Copy the `id` column into `attach`:

```sh
ariadne attach <id>
ariadne attach <id> --agent codex-acp   # two agents hold the same internal id
```

`attach` takes a session id, a task id, a goal id, or an outside session's
internal id — whichever the table showed. Point it at an outside id and
Ariadne resumes the conversation and opens the console on it in one step, no
separate command in between. Point it at an Ariadne session that has ended —
its goal completed, or its worktree already removed — and Ariadne revives it
the same way: a new agent process picks the stored conversation back up. `id`
alone is usually enough; `--agent` breaks the tie on the rare internal id two
agents both happen to hold.

Either way you land in the console described in [Using the
CLI](cli.md#connect-to-a-session): read what happened, send the next prompt,
leave whenever you want. The agent keeps working after you do.

## What a resumed session is

A resumed outside session has no goal, no task and no seat: nobody plans it,
authors it, or reviews it. It has no worktree either, and no landing — the
agent keeps working in the directory the original conversation already used,
against your checkout as it stands, and nothing it does is merged, opened as
a pull request, or otherwise landed on your behalf. Resuming does not copy
the conversation or change what the agent already knows; it is the same
session, reached through Ariadne instead of whatever you used to reach it
before.

A revived Ariadne session is different: it keeps its goal and task, and its
worktree if the daemon has not removed it yet. Reviving only replaces the
agent process behind the session; nothing about the task's plan or review
changes.

Once resumed, the session is no longer an outside one. List it again and it
shows under its own row, not as an outside session a second time; `attach
<id>` still reaches it, now as the session id rather than the internal id. It keeps the first prompt it was listed with as its title.

## When it refuses

Listing asks every registry agent that advertises `session/list`, and reads
the files of every agent whose transcripts Ariadne knows where to find. An
agent that does neither — no such capability, and no reader for its files —
contributes no sessions to the table, and `ariadne doctor` names the
capability as missing for that agent — check there before assuming a
conversation does not exist. Resuming asks the session's own agent to load
it, over ACP's `loadSession`. An agent that does not support that capability
cannot resume anything, outside session or ended one: `ariadne doctor` names
it as missing for that agent too.
