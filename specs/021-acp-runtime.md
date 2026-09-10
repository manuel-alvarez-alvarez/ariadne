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
is pinned (011), what the session is briefed with (006), and the CLI-side ACP client
`ariadne _spawn` still carries for launches outside the daemon (007).

## Behavior

1. A session of kind `acp` never has a tmux session. The launcher routes its
   every launch — spawn or resume, whatever the seat — through the runtime:
   no pane is ever created for it, and killing the session touches no pane.
   The row's `tmux_session` column keeps its derived name, no pane ever
   holds it, and the author spawn asks tmux nothing at all. Shared paths
   outside that spawn — the pane claim of an orchestrator or reviewer
   spawn, the `has_session` asks of the resume paths and of task cleanup —
   still ask tmux about the stored name and find nothing; they retire with
   the tmux path itself.
2. The agent is a direct child of the daemon: `Config::acp_bin` (the
   contract's `acp` on `PATH` outside a test) run with the rest of the
   adapter's argv, the plan's environment, and the worktree as its working
   directory, on piped stdio. The spawn plan is still written to the run
   directory as the record of the launch.
3. The runtime speaks ACP version 1: `initialize` (refusing an agent that
   negotiates anything else), `session/new` — or `session/resume`, falling
   back to `session/load`, where the launch resumes a stored session — then
   `session/prompt`, reading `session/update` notifications throughout.
4. The model the session is pinned to is set through
   `session/set_config_option` on the option of category `model` — or, where
   the agent categorizes none, on the option whose id or name is `model`.
   The effort is set the same way, on the category `thought_level` with
   `effort`, `reasoning` and `thought_level` as the id-or-name fallback, and
   only when the pin carries one. An agent offering no matching option fails
   the launch rather than running on a default.
5. What the agent does becomes agent events on the existing ingestion path
   (012), in the ACP adapter's vocabulary (007): `session_start`,
   `user_prompt_submit`, tool calls as `pre_tool_use` and `post_tool_use`,
   `stop` with the turn's last assistant text, `compaction_update`,
   `permission_request` and `permission.replied`, `session.error`, and
   `session_end`. The events carry the launch id, so a replaced agent's last
   words are told from the live one's; the agent's own session id rides
   `session_start` and is recorded on the row.
6. ACP permission mode defaults to `auto` from daemon configuration, and a
   task may override it with `auto`, `ask` or `learn`. `auto` selects the
   first allowing option, then the first option, and cancels only an empty
   list. `ask` records the request in the console, raises session attention
   and blocks until console input selects an option. `learn` does the same on
   the first request for one repository, tool name and tool kind; an allowing
   answer is stored and later matching requests are selected automatically.
   A denial is not stored.
7. After the initial prompt's turn ends the agent stays up, the way a TUI
   stays at its prompt, and the runtime keeps serving what it sends. The
   child is reaped whenever it ends: its own exit retires the session
   (`session_end`, and `session.error` first if the protocol failed), and
   killing the session kills the child.
8. Liveness is the runtime's registry, not tmux: the spawn guards and the
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

## Known gap

- One prompt per launch: verdicts, nudges and agent messages still take the
  pane-shaped delivery (008, 018), which an `acp` session has no pane for.
  Later tasks route them through `session/prompt`.
- The orchestrator and reviewer seats of kind `acp` launch through the same
  runtime, but only the author seat is proven end to end.

## Sources

`crates/ariadne-daemon/src/acp.rs` (the runtime),
`crates/ariadne-daemon/src/launcher.rs` (the routing, liveness and kill),
`crates/ariadne-daemon/src/scheduler/sweeps.rs` (the liveness sweep),
`crates/ariadne-daemon/tests/common/acp.rs` (the scriptable stub agent).
