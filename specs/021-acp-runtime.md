---
id: acp-runtime
status: current
updated: 2026-09-12
areas: [daemon]
commits: []
tests:
  - crates/ariadne-daemon/tests/acp_runtime.rs
  - crates/ariadne-daemon/tests/acp_console.rs
  - crates/ariadne-daemon/tests/acp_discovery.rs
  - crates/ariadne-daemon/src/acp.rs
---

# ACP runtime

How the daemon runs an agent: as its own child process, driven over the Agent
Client Protocol.

`ariadned` spawns the agent executable with piped standard input and output,
speaks ACP version 1 over newline-delimited JSON-RPC, and reports what the
agent does through the daemon's one event ingestion path. Every seat of every
session runs this way — orchestrator, author and reviewer. The runtime is
`crates/ariadne-daemon/src/acp.rs`; the launch it consumes — the command and
the launch file — is described in 007.

## Scope

In: the child process and its lifecycle, the protocol conversation, the model
and effort pins, the events the runtime reports, permission requests, prompt
delivery, liveness, and the kill.

Out: the registry, discovery, and what a launch is made of (007), which model
is pinned (011), what the session is briefed with (006), the console a person
reads and types into (008), and the sweep that retires a row whose agent is
gone (009).

## Behavior

1. The agent is a direct child of the daemon. It runs the registry command
   the launch names (007), with the launch's environment, in the seat's
   working directory, on piped stdio. A launch for a session that already
   has an agent here kills that agent first: one seat, one agent.
2. The runtime speaks ACP version 1. It sends `initialize` and refuses an
   agent that negotiates any other version (see Known gap). It then opens
   the session with `session/new` — or, for a resume, `session/resume` where
   the agent advertises it and `session/load` where it only advertises
   loading — and passes the MCP servers of the launch file every time.
3. The model is set through `session/set_config_option` on the option of
   category `model`, or, where no option has that category, on the option
   whose id or name is `model`. The effort is set the same way, on category
   `thought_level` with `effort`, `reasoning` and `thought_level` as the
   id-or-name fallback, and only when the launch carries one. An agent that
   offers no matching option fails the launch rather than run on a default
   (see Known gap).
4. Every prompt the runtime sends is the system prompt, a blank line, and
   the text of the prompt. The first prompt of a launch is the one its launch
   file carries, if any. `user_prompt_submit` carries the whole as `prompt`,
   the text alone as `text`, and a `source`: `console` for console input
   (008), `daemon` for everything the daemon itself says.
5. What the agent does becomes agent events on the ingestion path (012):
   `session_start` with the agent's own session id, `user_prompt_submit`,
   tool calls as `pre_tool_use` and `post_tool_use`, `plan` with the entries
   of each ACP plan update, the turn's text (rule 6), `stop` with the stop
   reason, a completed `compaction_update`, `permission_request` and
   `permission.replied`, `session.error`, and `session_end`. Every event
   carries the launch id (007), and the agent's session id is recorded on
   the row.
6. A turn's text is stored once, whole, when the turn ends: `agent_thought`
   `{session_id, text}` where thought chunks arrived, then `agent_message`
   `{session_id, text}` where message chunks arrived, then `stop`. While the
   turn runs, each chunk goes live to the session's console stream alone
   (008) — `agent_message_chunk` and `agent_thought_chunk`, each
   `{session_id, text}` — and so does each non-terminal `tool_call_update`
   as `tool_call_update` `{session_id, tool_call_id, acp}`. None of the
   three is stored, and none reaches `/v1/events` or `/v1/events/stream`.
7. Tool call updates are merged per `toolCallId`: every field an update sets
   replaces the one on record, `content` included. `pre_tool_use` carries
   the call as it opened; `post_tool_use`, on a `completed` or `failed`
   update, carries the merged call under `acp` — `title`, `kind`, `status`,
   `content`, `locations`, `rawInput`, `rawOutput` — beside the same
   `tool_name` and `tool_input` as before. A live `tool_call_update` carries
   the call merged so far.
8. A running turn is cancelled with ACP `session/cancel`, sent while the
   `session/prompt` it interrupts is still in flight. The response then ends
   the turn as any other: the text so far stored, and `stop` with
   `stop_reason: cancelled`. Between turns there is nothing to cancel, and
   the runtime refuses.
9. ACP permission mode defaults to `auto` from daemon configuration, and a
   task may override it with `auto`, `ask` or `learn`. `auto` selects the
   first allowing option, then the first option, and cancels only an empty
   list. `ask` records the request in the console, raises session attention
   and blocks until console input selects an option. `learn` does the same on
   the first request for one repository, tool name and tool kind; an allowing
   answer is stored and later matching requests are selected automatically.
   A denial is not stored.
10. After a turn ends the agent stays up and the runtime keeps serving it.
   Everything the daemon says to the agent after the launch — a scheduler
   nudge, a review briefing, an agent message — is a `session/prompt`, sent
   at once between turns and queued in order behind a running one. Console
   input (008) arrives the same way, except that a pending `ask` takes it as
   the answer (rule 9): only a person's input ever answers a permission.
11. The child is reaped whenever it ends. Its own exit ends the session on
   the record: `session.error` first if the protocol failed, then
   `session_end`. Killing the session kills the child and retires the row.
12. A resume starts a fresh agent process on the stored conversation. The
   predecessor is reaped, and its exit takes neither the seat nor the row
   down.
13. Liveness is the runtime's own registry of running agents, and it always
    answers. After a daemon restart no child of the old daemon is running,
    and a revive reaches the stored conversation through a new process.

## Acceptance criteria

- An author runs end to end — the handshake in order, the worktree, the
  `ariadne` MCP server, the model and effort pins, the briefing behind the
  system prompt as the first prompt, the events in the store, the captured
  agent session id, and the agent still up after the turn
  (`acp_runtime.rs::an_acp_author_runs_on_daemon_stdio`).
- An orchestrator seat runs the same way, with the ariadne MCP server in
  `session/new` (`acp_runtime.rs::an_orchestrator_runs_on_the_registry_agent`).
- A reviewer seat runs the same way, in its detached worktree
  (`acp_runtime.rs::a_reviewer_runs_on_the_registry_agent`).
- Killing the session kills the agent process and retires the row
  (`acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`).
- An agent that dies mid-turn is reaped, and its session ends on the record
  (`acp_runtime.rs::a_dead_acp_agent_is_reaped_and_its_session_retired`).
- A resume loads the stored conversation on a fresh process, the instruction
  rides the new prompt, and the predecessor's exit takes nothing down
  (`acp_runtime.rs::resuming_an_acp_author_replaces_the_agent_and_keeps_the_session`).
- After a daemon restart a revive reaches the stored conversation through
  `session/load`, on an agent that advertises no `session/resume`
  (`acp_runtime.rs::a_stub_session_resumes_through_session_load_after_a_daemon_restart`).
- `auto` approves a permission request with the allowing option, and the ask
  and answer are events
  (`acp_runtime.rs::auto_approves_a_permission_request_with_the_allowing_option`),
  and the allowing option is selected wherever it stands
  (`acp.rs::the_allowing_option_is_selected_wherever_it_stands`).
- `ask` raises session attention and console input unblocks the turn
  (`acp_console.rs::ask_raises_attention_and_a_console_answer_unblocks_the_turn`).
- `learn` remembers an approval per repository across a daemon restart, and
  does not remember a denial
  (`acp_console.rs::learn_remembers_an_approval_per_repository_across_a_daemon_restart`).
- Console input reaches the agent and queues behind a running turn
  (`acp_console.rs::posted_input_reaches_the_agent_and_queues_behind_a_running_turn`).
- A scheduler nudge arrives at the agent as a `session/prompt`
  (`acp_runtime.rs::a_scheduler_nudge_arrives_at_the_stub_agent_as_a_prompt`).
- Message chunks reach the console stream before the turn ends
  (`acp_console.rs::a_console_stream_client_sees_message_chunks_before_the_turn_ends`),
  and so do thought chunks and a tool call's progress
  (`::thought_chunks_and_tool_call_progress_reach_the_console_stream_live`).
- After the turn the whole thought and message are stored once each, before
  the `stop`, and no chunk is
  (`acp_console.rs::the_snapshot_after_a_turn_holds_the_whole_thought_and_message_once`);
  neither `GET /v1/events` nor `/v1/events/stream` carries a chunk
  (`::the_events_listing_and_the_domain_stream_carry_no_chunk`).
- `post_tool_use` carries the call merged from every update
  (`acp_console.rs::post_tool_use_carries_the_tool_call_merged_from_every_update`).
- `user_prompt_submit` carries the typed text and its source
  (`acp_console.rs::console_input_is_reported_as_its_text_from_the_console`).
- A cancel ends the running turn as `cancelled`
  (`acp_console.rs::cancelling_a_running_turn_ends_it_as_cancelled`), and is
  refused between turns (`::cancel_with_no_turn_running_is_refused`).
- An option is found by its category, or by its id or name where no option
  has the category — the lookup the runtime shares with discovery
  (`acp_discovery.rs::model_and_effort_name_fallbacks_enter_the_discovered_catalog`).

## Known gap

Two refusals of rules 2 and 3 are proven only at discovery, not at launch:
an agent on another protocol version, and an agent with no model option.
Discovery rejects both (007), and no pin can name a model of a rejected
agent (011), so no test launches one.

## Sources

`crates/ariadne-daemon/src/acp.rs` (the runtime),
`crates/ariadne-daemon/src/acp_rpc.rs` (the JSON-RPC transport),
`crates/ariadne-daemon/src/launcher.rs` (the launch, liveness and kill),
`crates/ariadne-daemon/src/scheduler/mod.rs` (the prompt delivery),
`crates/ariadne-daemon/tests/common/acp.rs` (the scriptable stub agent).
