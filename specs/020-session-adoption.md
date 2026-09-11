---
id: session-adoption
status: current
updated: 2026-09-11
areas: [api, daemon, cli]
commits: []
tests:
  - crates/ariadne-daemon/tests/session_adoption.rs
  - crates/ariadne-daemon/tests/acp_session_adoption.rs
---

# Session adoption

Ariadne can take over a conversation that started outside it: a supported
CLI's own transcript store, or an ACP agent's own stored sessions.

## Scope

In: finding outside Claude Code, Codex and OpenCode sessions from their
transcript stores; finding the stored sessions of an ACP agent that can list
them; showing a session's resume identity; and assigning one to a ready task
as its author.

Out: changing a task's author pin, importing a transcript, sessions that
Ariadne started itself (007, 008, 021), and the model and effort an ACP
agent's session runs at (011).

## Behavior

1. Discovery reads Claude Code JSONL files under `~/.claude/projects`, Codex
   rollout JSONL files under `~/.codex/sessions`, and the OpenCode SQLite
   store under `~/.local/share/opencode`.
2. Discovery also asks every ACP agent the registry (021's discovery task)
   found ready and advertising the `session_list` capability, over
   `session/list`. An agent without it — rejected outright, or ready but
   missing the capability — contributes no sessions; `GET /v1/acp-agents`
   still names why, in `rejection_reason` or the `no_adoption` degradation.
3. Each discovered session names its CLI (or, for an ACP one, `acp` and the
   registry agent id it belongs to), internal session id, working directory,
   first user prompt (an ACP session's title), and last activity time.
4. A session whose CLI (and, for `acp`, registry agent id) and internal id
   already occur in an Ariadne session row is not an outside session and is
   never listed. An `acp` session row's registry agent id is read off its
   `model` column, which an author's pin always writes as `<agent
   id>:<model>`.
5. `GET /v1/outside-sessions` lists every outside session found either way.
   The endpoint asks the stores and the agents when called; it does not
   persist copies of what either says.
6. `POST /v1/tasks/{id}/author-session` accepts an outside session's CLI (and,
   for `acp`, its registry agent id) and internal session id. It refuses a
   session that discovery no longer finds.
7. Adoption is valid only for a `ready` task, and the outside session's CLI
   must match the task author's pinned CLI — and, where that CLI is `acp`,
   its registry agent id must match the one the author's pin names.
8. Adoption creates the normal author row and task worktree, records the
   outside internal id, and launches the adapter resume path in that
   worktree: the CLI's own resume flag for Claude Code, Codex and OpenCode, or
   the ACP runtime's `session/load` (021) for `acp` — which claims no pane,
   the way every `acp` author launch does. The task then follows its normal
   author and review lifecycle, and an `acp` author is reached afterwards the
   way any of its later prompts are: `POST /v1/sessions/{id}/console/input`
   (021).
9. `ariadne session discover` lists outside sessions from both sources, an
   ACP one named by its registry agent id rather than by `acp` alone, and
   below the table, one line per ACP agent that cannot list its sessions,
   naming why. `ariadne session adopt <session-id> <task-id> --agent <cli>
   [--acp-agent <id>]` adopts one and prints its author session; `--acp-agent`
   is required alongside `--agent acp`.

## Acceptance criteria

- Hand-started Claude Code, Codex, and OpenCode sessions appear with their
  recorded fields (`session_adoption.rs::hand_started_sessions_from_each_cli_appear_in_the_listing`).
- An Ariadne session is not listed as outside
  (`session_adoption.rs::a_session_ariadne_started_does_not_appear_in_the_listing`).
- Adoption creates an author session through the adapter resume path and the
  task can move to review
  (`session_adoption.rs::an_adopted_session_authors_the_task_through_review`).
- An ACP agent's stored sessions appear in the listing, named by their
  registry agent id
  (`acp_session_adoption.rs::an_acp_agents_stored_sessions_appear_in_the_listing`).
- An ACP agent without the `session_list` capability lists no sessions, and
  the catalog says why
  (`acp_session_adoption.rs::an_agent_without_the_capability_lists_nothing_and_shows_the_reason`).
- Adopting a listed ACP session binds it to the author seat, claims no pane,
  resumes it through `session/load`, and a later console prompt reaches the
  same agent
  (`acp_session_adoption.rs::an_adopted_session_binds_the_seat_and_a_follow_up_prompt_reaches_it`).
- Adoption is refused across ACP agents, the same way it is refused across
  CLIs (`acp_session_adoption.rs::adoption_is_refused_across_acp_agents`).
- An adopted ACP session is not listed as outside again
  (`acp_session_adoption.rs::an_adopted_acp_session_no_longer_appears_in_the_listing`).

## Sources

`crates/ariadne-daemon/src/outside_sessions.rs` (the CLI transcript stores),
`crates/ariadne-daemon/src/acp_sessions.rs` (the ACP agents),
`crates/ariadne-daemon/src/acp_discovery.rs` (`AgentRegistry::stored_sessions`),
`crates/ariadne-daemon/src/http/sessions.rs`,
`crates/ariadne-daemon/src/http/tasks.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-cli/src/commands/session.rs`.
