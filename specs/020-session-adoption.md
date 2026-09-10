---
id: session-adoption
status: current
updated: 2026-09-10
areas: [api, daemon, cli]
commits: []
tests:
  - crates/ariadne-daemon/tests/session_adoption.rs
---

# Session adoption

Ariadne can take over a conversation that a supported CLI started outside it.

## Scope

In: finding outside Claude Code, Codex and OpenCode sessions, showing their
resume identity, and assigning one to a ready task as its author.

Out: changing a task's author pin, importing a transcript, and sessions that
Ariadne started itself (007, 008).

## Behavior

1. Discovery reads Claude Code JSONL files under `~/.claude/projects`, Codex
   rollout JSONL files under `~/.codex/sessions`, and the OpenCode SQLite
   store under `~/.local/share/opencode`.
2. Each discovered session names its CLI, internal session id, working
   directory, first user prompt, and last activity time.
3. A session whose CLI and internal id already occur in an Ariadne session
   row is not an outside session and is never listed.
4. `GET /v1/outside-sessions` lists outside sessions. The endpoint reads the
   stores when called; it does not persist copies of their transcripts.
5. `POST /v1/tasks/{id}/author-session` accepts an outside CLI and internal
   session id. It refuses a session that discovery no longer finds.
6. Adoption is valid only for a `ready` task, and the outside session CLI
   must match the task author's pinned CLI.
7. Adoption creates the normal author row and task worktree, records the
   outside internal id, and launches the adapter resume path in that
   worktree. The task then follows its normal author and review lifecycle.
8. `ariadne session discover` lists outside sessions. `ariadne session adopt
   <session-id> <task-id> --agent <cli>` adopts one and prints its author
   session.

## Acceptance criteria

- Hand-started Claude Code, Codex, and OpenCode sessions appear with their
  recorded fields (`session_adoption.rs::hand_started_sessions_from_each_cli_appear_in_the_listing`).
- An Ariadne session is not listed as outside
  (`session_adoption.rs::a_session_ariadne_started_does_not_appear_in_the_listing`).
- Adoption creates an author session through the adapter resume path and the
  task can move to review
  (`session_adoption.rs::an_adopted_session_authors_the_task_through_review`).

## Sources

`crates/ariadne-daemon/src/outside_sessions.rs`,
`crates/ariadne-daemon/src/http/sessions.rs`,
`crates/ariadne-daemon/src/http/tasks.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-cli/src/commands/session.rs`.
