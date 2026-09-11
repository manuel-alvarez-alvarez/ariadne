---
id: session-adoption
status: current
updated: 2026-09-11
areas: [api, daemon, cli]
commits: []
tests:
  - crates/ariadne-daemon/tests/acp_session_adoption.rs
  - crates/ariadne-cli/src/commands/session.rs
---

# Session adoption

Ariadne can take over a conversation that started outside it: a session an
ACP agent stored itself, which a ready task can take as its author.

## Scope

In: finding the stored sessions of the registry agents that can list them,
what a listed session says about itself, and assigning one to a ready task as
its author.

Out: changing a task's author pin (011), importing a transcript, sessions
that Ariadne started itself (007, 021), and the desktop screen over this
(015).

## Behavior

1. Discovery asks every registry agent that the last discovery run (007)
   found ready and advertising `session_list`, over `session/list`. Each is
   one short-lived process, the same shape as a probe. An agent without the
   capability — rejected outright, or ready but lacking it — contributes no
   sessions, and `GET /v1/acp-agents` says why, in `rejection_reason` or the
   `no_adoption` degradation. An agent that fails to answer contributes
   nothing rather than failing the listing.
2. Each listed session is an `OutsideSessionDto`: the registry `agent_id` it
   belongs to, its `internal_session_id`, its `working_directory`, its
   `last_activity_at`, and its title as `first_prompt`.
3. A session whose agent and internal id already occur on an Ariadne session
   row is not an outside session and is never listed. A row's agent is read
   off its `model` column, which always holds `<agent>:<model>`.
4. `GET /v1/outside-sessions` lists every outside session. It asks the
   agents when called and persists nothing of what they say.
5. `POST /v1/tasks/{id}/author-session` takes `{agent_id,
   internal_session_id}`. It refuses a session discovery no longer lists.
6. Adoption is valid only for a `ready` task whose author is pinned to the
   same agent the session belongs to; anything else is a conflict. An agent
   session may adopt only into its own task.
7. Adoption creates the normal author row and task worktree, records the
   outside internal id, and resumes that conversation on the pinned agent
   (007, 021) — through `session/load` on an agent that offers no
   `session/resume`. The task then follows its normal author and review
   lifecycle, and the author is reached afterwards the way any session is:
   `POST /v1/sessions/{id}/console/input` (008).
8. `ariadne session discover` lists the outside sessions, and below the
   table one line per registry agent that cannot list its sessions, naming
   why. `ariadne session adopt <session-id> <task-id> --agent <agent-id>`
   adopts one and prints its author session.

## Acceptance criteria

- An agent's stored sessions appear in the listing with their recorded
  fields, named by their registry agent id
  (`acp_session_adoption.rs::an_acp_agents_stored_sessions_appear_in_the_listing`).
- An agent without `session_list` lists no sessions, and the catalog says why
  (`acp_session_adoption.rs::an_agent_without_the_capability_lists_nothing_and_shows_the_reason`).
- Adopting a listed session binds it to the author seat, resumes it through
  `session/load` rather than `session/resume`, and a later console prompt
  reaches the same agent
  (`acp_session_adoption.rs::an_adopted_session_binds_the_seat_and_a_follow_up_prompt_reaches_it`).
- An adopted session is not listed as outside again
  (`acp_session_adoption.rs::an_adopted_acp_session_no_longer_appears_in_the_listing`).
- Adoption is refused across agents
  (`acp_session_adoption.rs::adoption_is_refused_across_acp_agents`), and an
  agent session cannot adopt into another task
  (`::an_agent_cannot_adopt_a_session_for_another_task`).
- `session discover` names each agent that cannot list its sessions, with its
  reason
  (`commands/session.rs::an_agent_without_the_capability_is_named_with_its_reason`).

## Known gap

No test adopts into a task that is not `ready`, or names a session discovery
no longer lists. Both refusals are in `Launcher::adopt_author` and
`http/tasks.rs::adopt_author_session`.

## Sources

`crates/ariadne-daemon/src/acp_sessions.rs` (the outside listing),
`crates/ariadne-daemon/src/acp_discovery.rs` (`AgentRegistry::stored_sessions`),
`crates/ariadne-daemon/src/http/sessions.rs`,
`crates/ariadne-daemon/src/http/tasks.rs`,
`crates/ariadne-daemon/src/launcher.rs` (`adopt_author`),
`crates/ariadne-cli/src/commands/session.rs`.
