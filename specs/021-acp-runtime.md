---
id: acp-runtime
status: current
updated: 2026-09-11
areas: [daemon]
commits: []
tests:
  - crates/ariadne-daemon/tests/acp_runtime.rs
  - crates/ariadne-daemon/src/acp.rs
---

# ACP runtime

How the daemon runs an agent of kind `acp`: as its own child process, driven
over the Agent Client Protocol, with no tmux session anywhere in its life.

Every other agent kind runs a CLI in a tmux pane (008). The `acp` kind is the
first to run inside the daemon instead: `ariadned` spawns the agent
executable with piped standard input and output, speaks ACP version 1 over
newline-delimited JSON-RPC, and reports what the agent does through the same
event ingestion its hooks would use. The runtime is
`crates/ariadne-daemon/src/acp.rs`; the launch it consumes is the ACP
adapter's own plan (007), `acp.json` included.

## Scope

In: the child process and its lifecycle, the protocol conversation, the model
and effort pins, event persistence, liveness, and the kill.

Out: what the adapter plans and how `acp.json` is spelled (007), which model
is pinned and how discovery catalogs it (011), what the session is briefed
with (006), and the CLI-side ACP client
`ariadne _spawn` still carries for launches outside the daemon (007).

## Behavior

1. A session of kind `acp` never has a tmux session. The launcher routes its
   every launch — spawn, resume, revive or retry, whatever the seat:
   orchestrator, author or reviewer — through the runtime: no pane is ever
   created for it, no pane is claimed for it, and killing the session
   touches no pane. The row's `tmux_session` column keeps its derived name,
   no pane ever holds it, and a spawn asks tmux nothing at all. Shared paths
   outside the spawns — the `has_session` asks of the resume paths and of
   task cleanup — still ask tmux about the stored name and find nothing;
   they retire with the tmux path itself.
2. The agent is a direct child of the daemon, and the pin picks it by
   registry id: a session's model of `<agent-id>:<model>` — the id of a
   discovered catalog entry (011) — runs that registry agent's command, and
   the bare model half is what the runtime pins. A model naming no registry
   agent runs `Config::acp_bin` (the contract's `acp` on `PATH` outside a
   test) with the model as pinned. Either command runs with the rest of the
   adapter's argv, the plan's environment, and the worktree as its working
   directory, on piped stdio. The spawn plan is still written to the run
   directory as the record of the launch, its argv the resolved command.
3. The runtime speaks ACP version 1: `initialize` (refusing an agent that
   negotiates anything else), `session/new` — or `session/resume`, falling
   back to `session/load`, where the launch resumes a stored session — then
   `session/prompt`, reading `session/update` notifications throughout.
   Every seat's session is opened with the MCP servers its config carries;
   the ariadne server is among them for every seat, the orchestrator's
   included.
4. A resume of a registry agent is gated on what discovery measured: an
   agent whose cached capabilities lack `session_load` is refused before any
   process starts, and the refusal reports the session as not resumable. A
   session on the `Config::acp_bin` fallback has no snapshot to read and is
   left to the runtime's own protocol errors.
5. The model the session is pinned to is set through
   `session/set_config_option` on the option of category `model` — or, where
   the agent categorizes none, on the option whose id or name is `model`.
   The effort is set the same way, on the category `thought_level` with
   `effort`, `reasoning` and `thought_level` as the id-or-name fallback, and
   only when the pin carries one. An agent offering no matching option fails
   the launch rather than running on a default.
6. What the agent does becomes agent events on the existing ingestion path
   (012), in the ACP adapter's vocabulary (007): `session_start`,
   `user_prompt_submit`, tool calls as `pre_tool_use` and `post_tool_use`,
   `stop` with the turn's last assistant text, `compaction_update`,
   `permission_request` and `permission.replied`, `session.error`, and
   `session_end`. The events carry the launch id, so a replaced agent's last
   words are told from the live one's; the agent's own session id rides
   `session_start` and is recorded on the row.
7. ACP permission mode defaults to `auto` from daemon configuration, and a
   task may override it with `auto`, `ask` or `learn`. `auto` selects the
   first allowing option, then the first option, and cancels only an empty
   list. `ask` records the request in the console, raises session attention
   and blocks until console input selects an option. `learn` does the same on
   the first request for one repository, tool name and tool kind; an allowing
   answer is stored and later matching requests are selected automatically.
   A denial is not stored.
8. After the initial prompt's turn ends the agent stays up, the way a TUI
   stays at its prompt, and the runtime keeps serving what it sends. The
   child is reaped whenever it ends: its own exit retires the session
   (`session_end`, and `session.error` first if the protocol failed), and
   killing the session kills the child. Everything the daemon says to the
   agent after the launch — a scheduler nudge, a review briefing, an agent
   message — arrives the same way: a `session/prompt`, sent at once between
   turns and queued in order behind a running one (009). Console input (008)
   does too, except that a pending `ask` consumes it as the answer (rule 7):
   only a person's input ever answers a permission, never a delivery.
9. Liveness is the runtime's registry, not tmux: the spawn guards and the
   scheduler ask it for `acp` sessions, and it always answers — there is no
   "could not be asked". A daemon restart answers no for every child of the
   daemon that died, and the liveness sweep retires such rows (009).

## Acceptance criteria

- An `acp` author runs end to end — launch, handshake, worktree, MCP, model
  and effort pins, briefing as the first prompt, events in the store, the
  captured agent session id — with no tmux session
  (`acp_runtime.rs::an_acp_author_runs_on_daemon_stdio_with_no_tmux_session`).
- Killing the session kills the agent process, and no pane is asked to die
  (`acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`).
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
- An agent that dies mid-turn is reaped and its session retired on the record
  (`acp_runtime.rs::a_dead_acp_agent_is_reaped_and_its_session_retired`).
- A resume loads the stored conversation on a fresh agent process:
  `session/resume` names the stored id, the instruction rides the new
  prompt, the predecessor is reaped, and its exit takes neither the seat nor
  the row down
  (`acp_runtime.rs::resuming_an_acp_author_replaces_the_agent_and_keeps_the_session`).
- An orchestrator seat runs on the agent its pin names in the registry: the
  registry command, the bare model and effort halves pinned, the ariadne MCP
  server in `session/new`, and not one tmux call
  (`acp_runtime.rs::an_orchestrator_runs_on_the_registry_agent_with_no_tmux_session`).
- A reviewer seat runs the same way, in its detached worktree
  (`acp_runtime.rs::a_reviewer_runs_on_the_registry_agent_with_no_tmux_session`).
- After a daemon restart a revive reaches the stored conversation through
  `session/load`, on an agent that advertises no `session/resume`
  (`acp_runtime.rs::a_stub_session_resumes_through_session_load_after_a_daemon_restart`).
- A session of an agent discovery measured without `session_load` is refused
  as not resumable, with no process started
  (`acp_runtime.rs::a_session_of_an_agent_without_session_load_is_not_resumable`).
- A scheduler nudge arrives at the agent as a `session/prompt`, off the
  keystroke path
  (`acp_runtime.rs::a_scheduler_nudge_arrives_at_the_stub_agent_as_a_prompt`).

## Sources

`crates/ariadne-daemon/src/acp.rs` (the runtime),
`crates/ariadne-daemon/src/launcher.rs` (the routing, liveness and kill),
`crates/ariadne-daemon/src/acp_discovery.rs` (the registry a pin picks by),
`crates/ariadne-daemon/src/scheduler/delivery.rs` (the prompt delivery),
`crates/ariadne-daemon/src/scheduler/sweeps.rs` (the liveness sweep),
`crates/ariadne-daemon/tests/common/acp.rs` (the scriptable stub agent).
