---
id: mcp-tool-surface
status: current
updated: 2026-09-19
areas: [mcp, cli]
commits: [b21bd69e, 20d998bc, 09955c22, 305ad2fb, a69b953f, 03f9c8b7, 29e6d84e, 1b09ac10]
tests:
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-daemon/tests/it/adapters.rs
---

# MCP tool surface

The only way an agent reaches Ariadne: a stdio MCP server, started per
session, that serves the tools of that session's seat and nothing else.

## Scope

In: the server's identity and instructions, the tools of each seat, seat
filtering, session scoping, and how a refusal reads.

Out: what an agent is told to do with each tool — that is the seat's playbook
(003, 004, 005).

## Behavior

1. `ariadne mcp serve` is spawned by the agent, which the daemon's ACP
   runtime hands it as an MCP server on `session/new`, `session/load` and
   `session/resume` (021). It reads its identity from the environment —
   session, seat, goal and, for a task session, the task — and proxies to the
   daemon's REST API with a session header, so the daemon enforces the scoping
   itself.
2. The server's instructions, which every session receives before its first
   prompt, say what this session is and carry the session rules that hold for
   every seat alike (006).
3. Whether anyone answers a question is the one rule picked by seat: the
   orchestrator's user answers in the console, one question at a time, and the
   orchestrator then waits; an author or reviewer works alone, and asks only
   where the task cannot go on without the answer. The rule names no channel —
   the seat's own playbook already says how to ask.
4. Tools are filtered by seat both in the listing and on the call, so a tool a
   seat may not use is one it never sees:
   - **orchestrator**: `get_task`, `create_task` (staffing the authors as a
     list — one for most tasks, several where the reviewers pick a winner
     (004) — and every agent it staffs names its model: 011), `update_task`
     (which replaces the author list whole through `authors`, and refuses
     `default` as a model — a model is required, and `default` stays legal
     for the effort alone), `list_models` (which holds only the models it
     can staff an agent on, each with its `agent_id`, and narrows to one
     `agent_id` on request: 011),
     `list_skills`, `finalize_plan`, `list_tasks`, `retry_task`,
     `cancel_task`, `complete_goal` — the last four are what it supervises the
     goal with once the plan is under way (003)
   - **author**: `get_task`, `request_review`, `fail_task`, `finish_task`,
     `record_pull_request`
   - **reviewer**: `get_task`, `get_diff` (which takes an `author` on a task
     staffed with several, naming whose branch to read), `submit_verdict`
     (an `author` likewise, naming whose change the verdict judges — and
     required there, since several reviews are open at once), `pick_winner`
     (the pick of spec 004: once per reviewer, only once every author is
     approved)
   - **every seat**: `send_message`, `read_messages` — the channel
     the agents talk to each other on (018); `save_memory`, `search_memory` —
     the repository facts agents choose to keep and retrieve (019);
     `search_code`, `outline`, `symbol`, `path`, `impact`, `repo_map` — the
     symbol index over the
     repositories and the graph over it (022), listed and served only while
     the daemon runs with `knowledge_enabled`, which every launch tells the
     server in `ARIADNE_KNOWLEDGE_ENABLED`. `impact` with neither `symbol`
     nor `diff` is the reviewer's own task diff, base branch to task branch,
     and a refusal for any other seat. `repo_map` with no `repository` maps
     every repository of the session's goal, each on a share of the budget
     and under a heading of its own. `symbol`, `path` and `impact` head each
     repository's hits with the repository's path, read from
     `GET /v1/repositories` once per call, and put another repository's
     hits — a caller across a route, a reference by name — under a heading
     of that repository's own
5. A call to a tool outside the seat's list is refused by name rather than
   forwarded.
6. A tool with no task in scope takes the session's own task, and refuses with
   an instruction to pass one where there is neither.
7. The daemon's 4xx refusal — a transition the task cannot make, a reviewer a
   session is not assigned as — comes back as bad parameters carrying the
   daemon's sentence, not as a server failure.
8. The tool listing carries cache hints (fresh for 0 ms, private to this
   session), because clients of protocol 2026-07-28 reject a listing without
   them and the tools then silently never load.
9. `list_skills` lists only skills that can staff a task agent, and gives the
   name and the one-line summary of each one and nothing else. The document
   is what the agent staffed on the skill reads; an orchestrator choosing
   between skills reads the line. The orchestrator's own skill stays
   available through the API and CLI, but is not a task staffing choice.
10. `read_messages` reads an inbox, not a transcript: a default read takes
    delivery of what is addressed to this session's own agent, and `all`
    reads the whole thread instead (018). The channel is the session's task,
    or its goal where it has no task. The tool's description says both ways.
11. `send_message` takes the address an agent writes rather than the one
    address it could have written. `to` takes the id of a staffed agent,
    `orchestrator`, or the seat word `author` or `reviewer` where one agent
    sits in that seat; the body is taken as `body` or as `message`. A refusal
    names every id with the seat it sits in, so the sender picks a reader
    rather than guessing again.
12. A value the schema offers is a value the tool takes. The schema an agent
    reads is derived from the parameter types, and the value it sends back is
    deserialized from them, so a spelling set on one of the two alone offers
    a word and then refuses it.

## Acceptance criteria

- Every launch hands the agent the `ariadne mcp serve` server and the
  session context it reads
  (`adapters.rs::a_spawn_plans_a_new_session_briefed_and_pinned`,
  `::every_launch_carries_the_session_context`).
- `list_models` holds only the models an agent can be staffed on, narrowed to
  an `agent_id` on request
  (`tools.rs::the_catalog_an_agent_sees_holds_only_the_models_it_can_be_staffed_on`).
- Every seat has the tools its playbook names and no others
  (`mcp.rs::every_seat_has_the_tools_its_playbook_names_and_no_others`), and
  every allowed tool is one the router actually serves
  (`::every_allowed_tool_is_one_the_router_serves`).
- Every session is told how Ariadne is reached
  (`mcp.rs::every_session_is_told_how_ariadne_is_reached`), only the
  orchestrator is told to ask, and an author or reviewer is told to work
  alone and ask only where the task cannot go on without the answer
  (`::only_the_orchestrator_is_told_to_ask`), and no session is told of a
  conversation (`::no_session_is_told_of_a_conversation`).
- The shared rules stay small (`mcp.rs::the_shared_rules_stay_small`).
- Every text the server hands an agent — instructions and tool descriptions —
  is Simplified Technical English
  (`mcp.rs::every_text_the_server_hands_an_agent_is_simplified_technical_english`).
- A refused call reaches the agent in the daemon's words
  (`mcp.rs::a_refused_call_reaches_the_agent_in_the_daemons_words`).
- The skill catalog excludes orchestrator-only skills, and gives a name and a
  summary per skill and nothing else
  (`tools.rs::the_skill_catalog_excludes_orchestrator_only_skills`).
- A default `read_messages` takes delivery, and `all` reads the whole thread
  (`tools.rs::a_default_read_takes_delivery_and_all_reads_the_whole_thread`);
  a session with no task reads the channel of its goal
  (`::the_orchestrator_reads_the_channel_of_its_goal`).
- A seat word addresses the one agent that sits in it, a seat with several is
  refused with their ids, and an address that names nobody is refused with
  every id and its seat
  (`tools.rs::a_seat_word_addresses_the_one_agent_that_sits_in_it`).
- A message body written as `message` is taken
  (`tools.rs::a_message_body_is_taken_as_message_too`).
- Every permission mode the schema offers is one the tool takes
  (`tools.rs::a_task_takes_every_permission_mode_its_schema_offers`).
- Memory tools save and search the repository they name
  (`tools.rs::memory_tools_save_and_search_the_named_repository`).
- Memory tools default to a task's repository
  (`tools.rs::memory_tools_default_to_the_task_repository`), default to a
  goal's only repository
  (`::memory_search_defaults_to_the_goals_only_repository`), and require a
  repository when the goal has several
  (`::memory_search_needs_a_repository_when_the_goal_has_several`).
- The knowledge tools are listed to every seat, and to none while the
  knowledge base is off
  (`mcp.rs::the_knowledge_tools_are_not_listed_when_the_knowledge_base_is_off`);
  what each one sends and answers is spec 022's
  (`tools.rs::search_code_asks_the_daemon_with_its_filters_and_answers_one_line_per_hit`,
  `::outline_defaults_to_the_task_repository_and_lists_line_ranges`,
  `::symbol_groups_its_answer_under_a_heading_for_each_repository`,
  `::symbol_defaults_to_the_task_repository`,
  `::path_answers_one_line_per_hop_and_says_when_none`,
  `::impact_reads_the_task_diff_for_a_reviewer_that_names_nothing`,
  `::impact_needs_a_symbol_or_a_diff_from_a_seat_that_is_no_reviewer`,
  `::repo_map_maps_every_repository_of_the_goal_on_a_share_of_the_budget`,
  `::repo_map_takes_one_repository_with_the_path_to_rank_around`).

## Sources

`crates/ariadne-cli/src/commands/mcp.rs`,
`crates/ariadne-cli/src/commands/mcp/tools.rs`.
