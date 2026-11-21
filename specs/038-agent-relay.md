---
id: 038
status: current
updated: 2026-11-21
areas: [daemon]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/relay.rs
---

# Agent relay

## Scope

The relay owns one agent's stdio independently of daemon connections.
The daemon runtime in 021 remains unchanged.
Daemon launch integration, tmux, and herdr are outside this spec.

## Behavior

1. Start `ariadned relay --session ID --agent NAME --task ID [--home PATH] -- PROGRAM [ARG ...]`.
   The relay runs as a subcommand of the shipped daemon binary.
   Dispatch the subcommand before daemon configuration, telemetry, or database startup.
   It starts exactly one child, with inherited environment and working directory.
   It pipes stdin, stdout, and stderr.
   The child leads a separate process group.
   No shell interprets the command arguments.
   Clap errors exit 2; relay failures exit 1; completed relay operations exit 0.
   An agent failure is an exit frame, rather than a relay failure.
2. Resolve home through `--home`, `ARIADNE_HOME`, then `~/.ariadne`, as the client does.
   Use `HOME/relays/ID.sock` and `HOME/relays/ID.jsonl`.
   ID contains 1–64 ASCII letters, digits, hyphens, or underscores.
   Create the relay directory with mode 0700 and the journal with mode 0600.
   Never overwrite existing session files or unlink an occupied socket.
   The socket's parent directory restricts access to the home owner.
3. Use UTF-8 JSON objects separated by newlines in both socket directions.
   First send `{"type":"hello","protocol_version":1,"last_input_seq":0}` on every connection.
   This greeting precedes input and replay requests.
   `last_input_seq` reports the last durably accepted daemon input number, initially zero.
   It contains no output journal sequence.
   Clients must check its version before they send replay, acknowledgments, or ACP input.
   Disconnect without acknowledgment when the client cannot support the advertised version.
   The greeting is not journaled or acknowledged.
   The agent transport remains ACP JSON lines.
   Each complete stdout or stderr line becomes one output frame.
   An unterminated final line also becomes a frame.
   Remove the line terminator; preserve its other contents without parsing ACP.
   Frames have increasing `seq` values starting at 1.
   Stdout order and stderr order each match their source pipe.
   Across pipes, order follows observation by the relay.
   An output frame has this form:
   `{"type":"message","seq":1,"stream":"stdout","data":"{\"id\":1}"}`.
   Stderr uses `"stream":"stderr"` and never enters agent stdin.
4. Append every output frame to the journal and synchronize it before delivery.
   The journal uses the same JSON lines as the socket, plus acknowledgment records.
   Retain all frames until explicit stop or manual removal after completion.
   A disconnected or slow daemon does not block journal collection.
   The relay buffers frame indexes and payloads in memory for replay.
5. A connection starts with `{"type":"replay","after":0}`.
   `after` must not exceed the relay's durable acknowledgment.
   Start delivery after the durable acknowledgment, even when `after` is older.
   Zero is therefore a safe reconnect cursor when the caller lacks acknowledgment confirmation.
   Send all remaining frames in order, then stream new frames.
   Accept cumulative `{"type":"ack","seq":N}` only for frames delivered on this connection.
   Reject backward acknowledgments.
   Synchronize each acknowledgment before advancing the replay checkpoint.
   A disconnect before acknowledgment permits duplicate delivery, but never skips a frame.
   A new connection drains available complete frames from the previous connection before taking ownership.
   It then closes the previous handler and reports acceptance in its greeting.
   Malformed frames, invalid cursors, or invalid acknowledgments close that connection only.
6. The daemon assigns increasing input numbers within one session.
   Forward `{"type":"message","seq":1,"data":"{\"id\":1,\"result\":{}}"}` to agent stdin with one newline.
   Require replay first, and reject embedded CR or LF characters.
   Drop input numbers at or below `last_input_seq` without forwarding them again.
   Reserve stdin queue capacity before accepting a new input.
   Append `{"type":"input","seq":1}` to the journal and synchronize it before queueing that input.
   These records do not enter output replay or its sequence space.
   Advance `last_input_seq` only after successful persistence.
   A full or closed stdin queue closes the connection without accepting that input.
   The next greeting excludes rejected input, so the daemon can retry it safely.
   Accepted queued input survives disconnection and immediate connection replacement.
   A reconnecting daemon resends only input numbers above the greeting's accepted number.
   Forward messages without ACP changes.
   Permission requests remain journaled like any stdout message.
   Disconnection never closes agent stdin or supplies a permission answer.
7. Detect agent exit without reaping the agent.
   Send SIGKILL to the full process group before reaping, even when descendants have redirected output.
   Reap the direct child after the kill, then disarm process group cleanup.
   Send no kill after that reap, including during terminal reporting or shutdown.
   Drain both output pipes.
   Journal `{"type":"exit","seq":N,"code":7,"signal":null}` after all output.
   A signal exit has `code:null` and the numeric Unix `signal`.
   Stop agent execution immediately; retain the socket in the `exited` state.
   This terminal reporter waits for an acknowledgment of the exit frame, then exits 0.
   A later connection therefore receives outstanding output and the exit status.
   Remove the socket when the relay exits; retain the completed journal for inspection.
8. Accept `{"type":"stop"}` with or without replay in every protocol version.
   If the agent is unreaped, send SIGKILL to its full process group first.
   Reap the direct child after the kill, then disarm process group cleanup.
   If the relay already reaped the agent, send no further kill.
   Remove the socket and journal, then exit 0.
   Ctrl-C uses the same stop behavior.
   Register its interrupt listener before readiness and retain it across transport loop iterations.
9. Print a status line at startup and once per second on stdout.
   Include session, agent, task, state, last activity time, agent PID, and `ariadne attach ID`.
   States are `offline`, `connected`, and `exited`.
   Activity tracks agent output and exit using UTC timestamps.
   The status requires no daemon or terminal escape support.
   A thread of its own writes the status. A full or closed stdout never stops the transport.

10. An upgrade replaces the daemon executable.
    Each running relay keeps its existing code until its session stops.
    Never restart an agent or relay merely because a new daemon connects.
    A new daemon must support socket protocol versions from older relays that can still be running.
    Protocol version remains 1 because no daemon uses this protocol yet.
    Preserve version 1 semantics across subsequent executable upgrades.
    Introduce a new protocol version for incompatible changes.

## Acceptance criteria

- Rules 1–6: `journals_before_delivery_and_replays_only_unacknowledged_messages`
  proves child launch, socket traffic, durable output, cumulative acknowledgments, and ordered reconnect replay.
- Rules 3, 6, 7: `disconnected_agent_finishes_and_next_connection_receives_output_and_exit`
  proves permission replay, answer forwarding, stderr collection, final output, and exit acknowledgment.
  It also covers terminal reporting after the agent is reaped and its pipes close.
- Rule 9: `a_closed_status_pipe_does_not_stop_the_relay` starts with a closed status pipe.
  `a_full_status_pipe_does_not_stop_the_relay` starts with a full status pipe.
  Both prove continued input forwarding and stop handling.
- Rules 4, 7, 9: `offline_status_and_output_do_not_need_a_daemon`
  proves disconnected completion, journal replay, and status without a daemon.
- Rule 7: `agent_exit_kills_descendants_without_session_pipes` proves cleanup of descendants with redirected output after agent exit.
- Rules 7–8: `stop_after_agent_exit_keeps_descendants_stopped` covers stop after reaping the agent.
  `ctrl_c_after_agent_exit_keeps_descendants_stopped` covers Ctrl-C after reaping.
- Rule 8: `stop_kills_descendants_without_session_pipes` proves cleanup of redirected descendants after stop.
  `ctrl_c_kills_descendants_without_session_pipes` proves the same cleanup after Ctrl-C.
  `stop_kills_the_full_process_group` proves descendant termination through closure of its private socket.
  `journals_before_delivery_and_replays_only_unacknowledged_messages` also checks session-file removal.

- Rules 1–2: `session_paths_are_private_and_cannot_be_overwritten` proves path protection, permissions, and command exit codes.
- Rule 5: `reconnect_replaces_owner_and_invalid_ack_does_not_stop_agent` proves exclusive ownership and invalid acknowledgment isolation.
- Rules 4, 8: `stop_remains_available_when_daemon_does_not_read_output` proves stop under socket backpressure.

- Rules 3, 8: `first_socket_frame_identifies_protocol_before_replay` proves the versioned greeting and stop without replay.
- Rules 3, 5, 6: `immediate_replacement_preserves_durable_input_and_drops_duplicates` proves durable acceptance,
  immediate replacement without a wait, increasing input numbers, and duplicate suppression.
- Rules 3, 6: `rejected_input_is_not_reported_and_can_be_retried` proves queue rejection,
  an unchanged acceptance checkpoint, and safe retry.
  `invalid_input_does_not_advance_acceptance` proves validation failures leave the checkpoint unchanged.
- Rule 10: `executable_upgrade_preserves_running_relay_and_protocol` proves retained code and version 1 reconnect after executable replacement.

## Known gap

Relay process crashes and machine restarts do not restart the child or restore journal ownership.
Existing journal files prevent accidental reuse of a session ID.
Operators can inspect and remove those files after they confirm no relay owns the session.
Frame size and journal retention limits are not implemented.

## Sources

- `crates/ariadne-daemon/src/relay.rs`
- [ACP runtime](021-acp-runtime.md)
