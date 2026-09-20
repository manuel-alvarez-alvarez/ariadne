---
id: acp-runtime
status: current
updated: 2026-09-20
areas: [daemon]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_discovery.rs
  - crates/ariadne-daemon/src/acp.rs
  - crates/ariadne-daemon/tests/it/transcript_usage.rs
---

# ACP runtime

How the daemon runs an agent: as its own child process, driven over the Agent
Client Protocol.

`ariadned` spawns the agent executable with piped standard input and output,
speaks ACP version 1 over those pipes, and reports what the agent does through
the daemon's one event ingestion path. Every seat of every session runs this
way — orchestrator, author and reviewer. The runtime is
`crates/ariadne-daemon/src/acp.rs`; the launch it consumes — the command and
the launch file — is described in 007.

The wire is the `agent-client-protocol` crate's, the protocol's own Rust SDK.
The daemon owns the process — it spawns it, signals its process group and
reaps it — and hands the SDK the two pipes and nothing else
(`acp_transport`). Every request it sends is built from the SDK's typed v1
schema (`acp_calls`, `acp_schema`), so a field the protocol renames is a
compile error rather than a payload an agent ignores.

The daemon advertises no compaction. An agent compacts its own conversation
near the context limit and carries on, and the daemon asks for none — the
report said only that the agent was back at its prompt, which whatever it
does next says anyway. It was read when the daemon typed the compaction
itself and had to know when to let the agent go; that has not been true
since (007, rule 17).

Both messages an agent sends are read as the JSON it sent, not as the SDK's
typed enums. `SessionUpdate` is a closed set, and the runtime has always
handled the update kinds it knows and let the rest by, which is what keeps a
kind added to the protocol from failing a live session; and a tool call is
stored as the agent wrote it, so whatever an adapter attaches to one reaches
the console.

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
   working directory, on piped stdio, as the leader of a process group of
   its own. A launch for a session that already has an agent here kills that
   agent first (rule 11), and starts once it is reaped: one seat, one agent.
   A launch after a kill of the session's agent that has not been reaped yet
   waits for that reap the same way.
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
   reason, `permission_request` and
   `permission.replied`, `session.error`, and `session_end`. Every event
   carries the launch id (007), and the agent's session id is recorded on
   the row.
   The `stop` event carries `ariadne_usage` with the launch id as its
   source: the launch's totals read from the agent's own transcript, a Codex
   rollout or a Claude Code transcript (012 rule 16). While a turn runs, the
   runtime reads the transcript again every `Timeouts::transcript_poll` and
   writes the totals to the store, so a long turn's figure moves. Where no
   transcript is found by the end of a turn, the launch uses the prompt
   responses instead: quota totals before standard usage, read as what that
   turn spent and added to the launch's earlier turns. A launch never
   reports both.
   A `usage_update` keeps its latest `used` and `size` on the session while
   the turn runs. Its `cost`, when present, is ignored.
6. A turn's text is stored run by run, where the agent wrote it. A run is
   the chunks of one kind in a row, and of one message where the chunks
   carry an ACP `messageId`; it ends at a chunk of the other kind, a chunk
   that names another `messageId` than the run's, a plan, a tool call or its
   update, a permission request, or the end of the
   turn, and it is stored then, once, whole —
   `agent_thought` or `agent_message` `{session_id, text}` — before whatever
   ended it. A turn that speaks around a tool call stores its text before the
   call and its text after it as two events, and the last run comes before
   `stop`; a turn that fails stores its last run before the error. Text that
   arrives between turns is not stored. While the
   turn runs, each chunk goes live to the session's console stream alone
   (008) — `agent_message_chunk` and `agent_thought_chunk`, each
   `{session_id, text}` — and so does each non-terminal `tool_call_update`
   as `tool_call_update` `{session_id, tool_call_id, acp}`. None of the
   three is stored, and none reaches `/v1/events` or `/v1/events/stream`.
7. Tool call updates are merged per `toolCallId`: every field an update sets
   replaces the one on record, `content` included. `pre_tool_use` carries
   the call as it opened, including its input. `post_tool_use`, on a
   `completed` or `failed` update, carries the compact merged call under
   `acp` — `title`, `kind`, `status`, `content`, and `locations` — beside its
   `tool_name`. It drops `rawInput` and `tool_input`. Where a `content` entry
   has text, it is the stored output and `rawOutput` is dropped; otherwise
   `rawOutput` remains. A `content` diff and locations remain. A live
   `tool_call_update` carries the call merged so far.
8. A running turn is cancelled with ACP `session/cancel`, sent while the
   `session/prompt` it interrupts is still in flight. The response then ends
   the turn as any other: the text so far stored, and `stop` with
   `stop_reason: cancelled`. Between turns there is nothing to cancel, and
   the runtime refuses. A cancel may name the launch it was decided on, and
   is then refused too where the session has been relaunched since. Each
   launch also reports its turns as they go — a tool call ended, by the name
   its `post_tool_use` carries, and the turn ended — to followers of its own,
   each on an unbounded channel so that no report is ever dropped, followed
   by session and launch and refused for a launch that is not the one
   running, so a report of the process before never passes for the current
   one. The turn an author asked for its review in is ended that way
   (004): its own launch's report of the review call ended, then the cancel,
   which never lands on the next launch's briefing.
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
   `session_end`. Killing the session kills the child and retires the row;
   the kill does not wait for the reap. Killing the child, and reaping it,
   sends the kill to its whole process group: an adapter that runs the agent
   in a process of its own — codex-acp's `codex app-server` — leaves no
   process behind that still writes the conversation.
   A kill that finds a turn running first sends `session/cancel`, starts no
   queued prompt, and waits up to five seconds for the turn's response, whose
   `stop` records what the turn spent; a turn waiting on a permission answer
   is not cancelled, and an agent that does not answer is killed when the
   wait runs out.
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
  (`acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`), and
  kills the processes the agent started
  (`acp_runtime.rs::killing_an_acp_session_kills_the_processes_its_agent_started`).
- A kill mid-turn cancels the turn and keeps what it spent
  (`acp_console.rs::a_turn_killed_mid_way_is_cancelled_and_keeps_what_it_spent`),
  a relaunch over a running turn starts once the old agent is reaped and
  keeps what the old launch spent
  (`::a_relaunch_over_a_running_turn_keeps_what_the_old_launch_spent`), a
  relaunch after a kill resumes once the killed agent and its writer are gone
  (`::a_relaunch_after_a_kill_resumes_once_the_killed_agent_is_gone`), and
  an agent that ignores the cancel is killed when the wait runs out
  (`::an_agent_that_ignores_the_cancel_is_killed_when_the_grace_runs_out`).
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
- Each run of text is stored once, whole, where the agent wrote it — before
  the plan, the tool call or the `stop` that ended it — and no chunk is
  (`acp_console.rs::the_snapshot_after_a_turn_holds_each_run_of_text_where_it_was_written`),
  each message the agent names is stored on its own
  (`acp_runtime.rs::each_message_the_agent_names_is_stored_on_its_own`),
  and an agent that dies mid-turn keeps the run it was writing
  (`acp_runtime.rs::an_agent_that_dies_mid_turn_keeps_the_text_it_was_writing`);
  neither `GET /v1/events` nor `/v1/events/stream` carries a chunk
  (`::the_events_listing_and_the_domain_stream_carry_no_chunk`).
- `post_tool_use` stores its output once, keeps the opening input on
  `pre_tool_use`, and is smaller than its uncompact form
  (`acp_console.rs::post_tool_use_stores_text_once_and_keeps_the_opening_input`).
- `user_prompt_submit` carries the typed text and its source
  (`acp_console.rs::console_input_is_reported_as_its_text_from_the_console`).
- A cancel ends the running turn as `cancelled`
  (`acp_console.rs::cancelling_a_running_turn_ends_it_as_cancelled`), and is
  refused between turns (`::cancel_with_no_turn_running_is_refused`); an
  author's review request draws one such cancel once the agent reports the
  call ended, keeps what the turn spent, and leaves the agent up for the
  verdict's prompt
  (`::an_authors_review_request_ends_its_turn_and_the_verdict_still_reaches_it`),
  and a launch's turn reports are its own: the process before reports to
  nobody but itself
  (`::a_prior_launchs_late_review_report_does_not_end_the_new_launchs_turn`);
  a burst of reports before the review call's loses none of them, and the
  cancel still follows
  (`::a_burst_of_reports_before_the_review_calls_loses_none_and_the_cancel_follows`).
- Prompt usage keeps cache writes in input and counts only cache reads as
  cached input, prefers quota, adds up one
  launch's turns, adds a resumed launch, and leaves an absent report at zero
  (`acp_console.rs::standard_prompt_usage_adds_up_a_launchs_turns_and_rolls_up`,
  `::quota_prompt_usage_takes_precedence_over_standard_usage`,
  `::a_prompt_without_usage_keeps_zero_totals_and_records_stop`,
  `::resumed_prompt_usage_adds_a_new_launch_total`).
- Context updates keep a session's current used and size figures before its
  turn ends, and ignore a reported cost
  (`acp_console.rs::context_updates_keep_the_sessions_window_current_while_it_runs`).
- A transcript's figure wins over the prompt response, moves while a turn
  runs, and adds up across launches
  (`transcript_usage.rs::a_codex_session_stores_its_rollouts_total_not_its_prompt_response`,
  `::a_claude_session_counts_each_request_once_with_its_subagents`,
  `::a_running_turns_figure_moves_before_its_stop`,
  `::two_launches_of_one_codex_session_add_up`,
  `::a_session_without_a_transcript_keeps_its_prompt_responses_figure`).
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
`crates/ariadne-daemon/src/acp_transport.rs` (the agent's pipes as the SDK's
transport), `crates/ariadne-daemon/src/acp_calls.rs` (the methods it calls),
`crates/ariadne-daemon/src/acp_schema.rs` (Ariadne's launch types as the
SDK's),
`crates/ariadne-daemon/src/transcript.rs` (the transcript reader),
`crates/ariadne-daemon/src/launcher.rs` (the launch, liveness and kill),
`crates/ariadne-daemon/src/scheduler/mod.rs` (the prompt delivery),
`crates/ariadne-daemon/tests/it/common/acp.rs` (the scriptable stub agent).
