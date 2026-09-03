---
id: mcp-tool-surface
status: current
updated: 2026-09-04
areas: [mcp, cli]
commits: [b21bd69e, 20d998bc, 09955c22, 305ad2fb]
tests:
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
---

# MCP tool surface

The only way an agent reaches Ariadne: a stdio MCP server, started per
session, that serves the tools of that session's role and nothing else.

## Scope

In: the server's identity and instructions, the tools of each role, role
filtering, session scoping, and how a refusal reads.

Out: what an agent is told to do with each tool — that is the role's playbook
(003, 004, 005).

## Behavior

1. `ariadne mcp serve` is spawned by the agent CLI with a config generated at
   session spawn. It reads its identity from the environment — session, role,
   goal and, for a task session, the task — and proxies to the daemon's REST
   API with a session header, so the daemon enforces the scoping itself.
2. The server's instructions, which every session receives before its first
   prompt, say what this session is and carry the session rules that hold for
   every role alike (006).
3. Whether anyone answers a question is the one rule picked by role: the
   planner's user answers in the terminal, one question at a time, and the
   planner then waits; an engineer or reviewer works alone and does not ask.
4. Tools are filtered by role both in the listing and on the call, so a tool a
   role may not use is one it never sees:
   - **planner**: `get_task`, `create_task`, `update_task`, `list_models`,
     `list_profiles`, `finalize_plan`
   - **engineer**: `get_task`, `request_review`, `fail_task`, `mark_merged`,
     `record_pull_request`
   - **reviewer**: `get_task`, `get_diff`, `submit_verdict`
5. A call to a tool outside the role's list is refused by name rather than
   forwarded.
6. A tool with no task in scope takes the session's own task, and refuses with
   an instruction to pass one where there is neither.
7. The daemon's 4xx refusal — a transition the task cannot make, a reviewer a
   session is not assigned as — comes back as bad parameters carrying the
   daemon's sentence, not as a server failure.
8. The tool listing carries cache hints (fresh for 0 ms, private to this
   session), because clients of protocol 2026-07-28 reject a listing without
   them and the tools then silently never load.

## Acceptance criteria

- Every role has the tools its playbook names and no others
  (`mcp.rs::every_role_has_the_tools_its_playbook_names_and_no_others`), and
  every allowed tool is one the router actually serves
  (`::every_allowed_tool_is_one_the_router_serves`).
- Every session is told how Ariadne is reached
  (`mcp.rs::every_session_is_told_how_ariadne_is_reached`), only the planner is
  told to ask (`::only_the_planner_is_told_to_ask`), and no session is told of a
  conversation (`::no_session_is_told_of_a_conversation`).
- The shared rules stay small (`mcp.rs::the_shared_rules_stay_small`).
- Every text the server hands an agent — instructions and tool descriptions —
  is Simplified Technical English
  (`mcp.rs::every_text_the_server_hands_an_agent_is_simplified_technical_english`).
- A refused call reaches the agent in the daemon's words
  (`mcp.rs::a_refused_call_reaches_the_agent_in_the_daemons_words`).

## Sources

`crates/ariadne-cli/src/commands/mcp.rs`,
`crates/ariadne-cli/src/commands/mcp/tools.rs`.
