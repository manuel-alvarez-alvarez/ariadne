---
id: sessions-terminals-and-logs
status: current
updated: 2026-09-12
areas: [daemon, store, cli]
commits: [e4816cf6, 39937143, a69b953f]
tests:
  - crates/ariadne-daemon/tests/resume.rs
  - crates/ariadne-daemon/tests/acp_console.rs
  - crates/ariadne-daemon/tests/acp_terminal.rs
  - crates/ariadne-daemon/tests/acp_runtime.rs
  - crates/ariadne-daemon/tests/events.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/console.rs
  - crates/ariadne-cli/src/commands/console/tui.rs
  - crates/ariadne-cli/src/commands/transcript.rs
  - crates/ariadne-console/src/ansi.rs
  - crates/ariadne-console/src/markdown.rs
  - crates/ariadne-console/src/transcript.rs
  - crates/ariadne-console/src/tui.rs
---

# Sessions and the console

An agent session is a row the daemon keeps for one agent on one seat, and the
agent behind it is a daemon-owned ACP child process (021). This is what a
session holds, what may be done to it, and how a client reads it and speaks
to it: through the session's console.

## Scope

In: the session row and its statuses, launches and relaunches of one row,
kill and resume, the console — its transcript, its live stream and posted
input — how the CLI reaches the console, and the console served as a
terminal over a WebSocket for the desktop app.

Out: the ACP runtime itself — the child process, the protocol conversation,
permission modes and the event vocabulary (021); sessions started outside
Ariadne and adopted as authors (020); when the daemon hands an agent a prompt
(009) and what the prompt says (006); how `ariadne attach` resolves a task or
goal id to a seat (014).

## Behavior

1. A session belongs to a goal, a seat and — for authors and reviewers — a
   task.
2. A session holds its worktree, the model it runs on, the effort where one
   was pinned, and the agent's own session id once the agent reports one.
3. The model is one `<agent>:<model>` pin (011), frozen off the seat's pin
   when the session is created. The agent is the registry id before the
   pin's first `:`, and no agent kind is stored beside it.
4. Every session's agent is an ACP child process that the daemon owns. No
   session has a terminal, a pane or a grid.
5. A session is `starting`, `running`, `idle`, `exited` or `failed`. The
   first three are live.
6. Sessions are long-lived: one author per task, one reviewer per task across
   every review of it, one orchestrator per goal. Restarting one reopens the
   same row.
7. Every launch of a session is dated, and every launch runs under a launch
   id of its own. The id is written to the row before the agent starts and is
   handed to the agent's MCP server as `ARIADNE_LAUNCH_ID`.
8. A report from a launch the row has moved past changes nothing (012). A
   relaunch has two processes under one session id for as long as the old
   one takes to exit, and only the launch id tells their reports apart.
9. A relaunch announces the session as updated on the event stream.
10. Killing a session kills its agent process and marks a live session
    `exited`. The conversation stays with the agent, so the session can be
    resumed.
11. Resuming a session revives it in place: the same row, the same id, the
    same model, on the conversation its agent id names.
12. A session with no agent id to resume from is not revived. A session whose
    goal is finished is not revived either, and it stays as it ended.
13. A session's console snapshot (`GET /v1/sessions/{id}/console`) is the
    session's events so far, in order. Every event the runtime reported
    passes through as the runtime named it, with no fixed list of kinds.
    While a turn runs, the snapshot holds the text so far: one
    `agent_thought_chunk` and one `agent_message_chunk`, each only where
    there is text. The text so far is read first, under the runtime's turn
    lock, and the stored events after it, and the snapshot is the two in id
    order: the chunks take fresh ids as they are read, so they fall after
    every event stored before the read and before every event stored after
    it — a turn that ended, or a prompt that began, between the two reads is
    in the snapshot, in its place, rather than missing from it and arriving
    later behind the text. A live event sent between the two reads — the last
    chunk of a turn that ended in between, the progress of a call that ended —
    has an id below the last stored event read, and joins the snapshot in its
    place too; one sent after the stored events were read follows the
    snapshot on the stream. A live channel that dropped events while the
    stored ones were read has lost text the snapshot cannot show: the
    snapshot goes out as it is, and the first thing the stream says after it
    is a resync (rule 15). `GET /console` has no stream to say resync on: it
    opens again while the channel lags under the open, and when the channel
    lags every time it refuses with 503 rather than answer short. Between
    turns the snapshot is the stored events alone.
14. The console stream (`GET /v1/sessions/{id}/console/stream`) opens with
    that snapshot, then sends each later event as it is recorded, and each
    live-only event as the runtime streams it (021): message and thought
    chunks, and tool call progress. The stored and the live events go out in
    the order the daemon gave them ids, which one monotonic generator gives
    both: whenever both channels hold events, they are sorted by id before
    they go out, so a stored whole never passes the live chunk that preceded
    it and a live chunk never passes the stored prompt that began its turn.
    A client that connects mid-turn reads the text so far in its snapshot and
    every later chunk on the stream, none of them twice. A stream that cannot
    read the store for the stored events a live event overtook does not send
    the live event out of its place: the client is told to resync, as when
    it fell behind (rule 15), with no count of events missed.
15. The console stream has no replay. A client that falls too far behind is
    told how many events it missed, and the connection closes, the same as
    `/v1/events/stream` (012). A reconnect starts again from a fresh snapshot.
16. Console input (`POST /v1/sessions/{id}/console/input`) becomes a
    `session/prompt`. An agent runs one turn at a time, so input posted while
    a turn runs is queued and sent the moment the turn ends, in the order it
    was posted.
17. Console input that answers a pending permission question is taken as the
    selected option (021).
18. Console input takes down whatever the session was flagged for: an answer
    is an answer, whoever gave it.
19. A finished session refuses console input with `409`, and so does a
    session whose agent process is gone.
20. `POST /v1/sessions/{id}/console/cancel` cancels the turn the session is
    running (021) and answers `204`. A session that is not live, one whose
    agent process is gone, and one between turns have nothing to cancel, and
    answer `409`.
21. A session's console is also served as a terminal
    (`GET /v1/sessions/{id}/console/terminal`), for a client that is a
    terminal emulator rather than a terminal: the desktop app's xterm.js.
    The request upgrades to a WebSocket; an id that names no session
    answers `404` before the upgrade. The daemon runs the console of rules
    22 to 29 itself, in process, on a ratatui backend of its own that writes
    the escape sequences a terminal reads and answers the cursor position
    and the size from what it wrote and what the client said, so no query
    ever goes to a terminal; the scrollback is the emulator's. The protocol
    is defined once, in `ariadne-api` (`TerminalClientMessage`,
    `TerminalServerMessage`). Client to server, as JSON text frames: a
    `resize` with `cols` and `rows`, sent first — nothing is drawn before it
    — and on every change, a size of no rows or no columns counting as one;
    a `key` with a `code` (a printable character as `{"char": "a"}`, a
    function key as `{"f": 5}`, and `enter`, `backspace`, `tab`, `back_tab`,
    `esc`, `left`, `right`, `up`, `down`, `home`, `end`, `page_up`,
    `page_down`, `delete`, `insert` by name) and its `modifiers` (`shift`,
    `control`, `alt`, `super`, `hyper`, `meta`), one to one onto crossterm's
    `KeyEvent`; and a `paste` with its `text`. Server to client: binary
    frames of the terminal bytes that draw the console, and a text frame
    `{"type": "status", "status": ...}` with the session's status as the
    socket opens and as the console ends. The console's events are the
    snapshot and the stored and live events of rules 13 and 14, from the
    same channels; a receiver that fell behind says the stream dropped and
    subscribes again from a fresh snapshot, as the CLI does. What is typed
    and Escape take the paths of rules 16 to 20, so a permission answer, a
    queued prompt and the attention flag behave the same whichever console
    it was. The console runs while the socket is open: the client closing
    it ends the console and frees the session's receivers, and the session
    stays alive; the session ending sends the last bytes, the final status
    and closes the socket, and a socket opened after the session ended
    draws the transcript and closes on it the same way; Ctrl-C twice and
    Ctrl-D over the socket close it too, and the session stays alive.
22. The CLI reaches a session through its console. `ariadne attach` renders
    the transcript, follows the stream and posts what is typed as input.
    `ariadne session logs` and `ariadne task logs` print the snapshot as typed
    transcript blocks. Paired tool and permission events form one block;
    agent text stays whole, and diffs retain their line colouring. `--tail`,
    `--since` and `--kind` filter the snapshot. With `--follow`, chunks stream
    under one item header and `--kind` also filters later events. JSON output
    keeps each event object unchanged.
23. `ariadne attach` on a terminal is an inline pane, never the alternate
    screen. A finished block goes into the terminal's own buffer above the
    pane, so it stays in the scrollback; the pane holds the block still being
    written, a status line and the input box. The status line names the seat,
    the model and the session's status — the row's at attach, then what the
    stored events move it to, as the daemon moves the row on them (021):
    `running` on the session's start, a prompt, a tool event or an answered
    permission, `idle` on a stop or a compaction, `exited` on the session's
    end. A row that had ended when the console attached — `exited` or
    `failed` — stays so whatever the events replayed under it say, since the
    daemon moves a live row only and a row its sweep ended has no
    `session_end` stored; a live row's replay may hold an earlier launch's
    `session_end` before this launch's `session_start`, and follows both, so
    a resumed session reads as the daemon has it. The status line also turns
    a spinner with "thinking" or
    "running &lt;tool&gt;" while a turn runs, followed by how long the turn
    has run — `12s`, or `1m 04s` past a minute — counted from the event that
    began it, so a turn already running at attach counts from its prompt;
    the count starts again with each turn, survives a reconnect's replay,
    and is absent between turns. Every block, the agent's markdown and the
    input box wrap and cut by display width, so a wide character or an emoji
    takes the two columns it draws on, and a cut falls between grapheme
    clusters, so an emoji of several characters is never split. A resize of
    the terminal redraws the pane at the new size. The pane opens from the
    cursor, which the terminal is asked for once, at the open; a terminal that
    does not answer within crossterm's timeout gets the same pane opened from
    the bottom row instead. On both paths every later cursor query — the one
    ratatui makes after each block it inserts above the pane, and on a resize
    — is answered by the backend itself, from where it last put the cursor,
    and never sent to the terminal: once the key stream reads the terminal, a
    query's answer would come through the reader the stream holds, and time
    out. There is no alternate-screen fallback.
24. The pane renders each block as it arrives: a prompt as `> text`, holding
    the event's `text` alone — never the whole `prompt` with the system
    prompt ahead of it (021), nor the summary, one line cut short; an event
    carrying no `text` says the text was not recorded rather than draw the
    whole. A prompt the daemon sent (`source: daemon`: a briefing,
    a nudge, a message) draws under its own marker and label, `» daemon`,
    with its text beneath, so it reads apart from what was typed. Agent text
    is markdown chunk by chunk under one marker, a thought dimmed and folded,
    a tool call the block of the next rule, a plan a checklist, and a
    permission question a picker.
25. A tool call reads as a coding agent's. Its head line is a status glyph —
    `○` pending, `●` in progress, `✓` completed, `✗` failed — then a glyph
    for the ACP `kind` (`$` execute, `≡` read, `✎` edit, `⌫` delete, `→`
    move, `⌕` search, `↓` fetch, `∴` think, `⇄` switch mode, `•` otherwise)
    and what the call is about, taken from its input: the command for
    `execute`, the path and line for `read`, `edit`, `delete` and `move`, the
    pattern and where it is looked for for `search`, the URL for `fetch`, and
    the call's title where the kind names nothing. Raw JSON is never drawn
    where a field names the subject, and `locations` the head does not already
    name follow it. Once the call has ended, the head carries the time from
    the event that opened it to the one that ended it. A live
    `tool_call_update` merges into the open call with the same `toolCallId`:
    one block per call, however many updates arrive. A call that has not
    ended stays in the pane, and so does everything after it: the question
    about a call comes after the call and the event that ends it after the
    answer, so the scrollback gets the call as it ended, never as pending.
    The output is the
    call's `rawOutput` where that is text or a stdout and stderr pair, the
    text of its `content` entries otherwise, and the structure as JSON only
    where there is neither. It is folded to its last lines with a count of
    the hidden ones, trailing blank lines trimmed; the fold is the thought's.
    A tab in the output takes the columns to the next stop of eight, since a
    cell drawn with a tab draws nothing. A `diff` content entry draws as a
    unified diff under a file header — the path, or the old name to the new
    where they differ — with added, removed, hunk and context lines each in
    their own colour, folded past a line limit with a count. Where the agent
    sent `oldText` and `newText` rather than a patch, the diff is the hunks
    between them with three lines of context, never every old line and then
    every new one. A patch that starts at its first hunk takes its file
    header from the entry's `path`. A permission question draws the call it asks about under
    the question — the same head line, then the rest of a command that has
    more than one line, or the diff — above its options.
26. A chunk continues the block last written, and starts a block of its own
    where anything else came between: a turn that speaks around a tool call
    reads as two blocks with the call between them. The whole text the daemon
    stores at the end of that turn (021) is every chunk of it joined, so it
    closes the blocks the chunks opened and repeats none of them. A turn that
    streamed nothing renders that stored text as its one block. A turn that
    ends at once has its last chunks and its stored whole ready together, and
    a stream can hand over the whole first: a chunk that arrives after the
    whole of its kind, with an id below that whole's, and says nothing the
    whole did not is a late chunk of that turn, folded in and not drawn
    again, and it does not reopen the block for the chunk after it. The id is
    what tells it from the next turn's first chunk handed over before its
    prompt: the daemon gives the live chunks and the stored events their ids
    from one monotonic generator, and a new turn's chunk gets its id after
    its prompt, which is after the whole before it.
27. Enter posts the input box to console input, Shift+Enter and Alt+Enter add
    a line to it, and each prompt typed shows at once and is replaced by its
    own `user_prompt_submit`, in the order they were posted. A prompt typed
    while a turn runs is queued behind it (rule 16), so what that turn says
    after the prompt was typed is drawn above the prompt — the prompt is the
    next turn's — and the block above the prompt stays in the pane while the
    turn writes into it. An older
    daemon's (021) prompt event carries neither `text` nor `source`: it takes
    the oldest pending prompt's place where its whole `prompt` ends in a
    blank line and then the typed text — the whole is the system prompt, a
    blank line and the text, so a daemon prompt that merely ends in the same
    words does not match — keeping what was typed, and is a prompt of its
    own otherwise. The terminal is in bracketed paste, so a paste arrives as
    one event: it goes into the box at the cursor with its line breaks kept,
    and sends nothing until Enter. The box has the shell's line-editing keys:
    Ctrl-A and Ctrl-E to the start and the end of the line, Ctrl-U and Ctrl-K
    deleting to them, Ctrl-W deleting the word before the cursor, and
    Alt-Left and Alt-Right — or Alt-B and Alt-F, which is what a terminal
    that sends the readline sequences for them gives — moving by word. On a
    permission question the arrows and the number keys move the pick and
    Enter posts the option's id. While it waits, the picker is the last block
    of the pane, above the box, whatever came after it — a snapshot taken
    mid-turn ends on the text so far (rule 13), which comes after the
    question in it — and its command or diff is folded to the room its
    question and options leave, so the question and every option are on the
    screen together. A post the daemon refuses is said on the transcript, and
    the console stays open.
28. Escape during a running turn posts to console cancel. Ctrl-C twice, or
    Ctrl-D, leaves the console, and the session stays alive. Every way out
    puts the terminal back: raw mode off, bracketed paste off and the cursor
    shown.
29. A dropped stream says "reconnecting" and is dialled again on the backoff
    every other follow uses. The fresh snapshot redraws what was open and does
    not repeat what is already in the scrollback.
30. With stdin or stdout redirected there is no pane. `ariadne attach` is then
    the plain line protocol: one `kind · summary` per event, numbered options
    for a permission question, and one prompt per line read (014).

## Acceptance criteria

- A session keeps the model it started on, however the seat's pin moves
  afterwards
  (`resume.rs::a_running_reviewer_keeps_the_model_its_session_started_on`,
  `::a_resumed_author_stays_on_the_model_its_session_started_on`).
- The schema names an agent by its registry id alone, and a session stores no
  agent kind (`store.rs::the_schema_names_agents_by_registry_id_alone`).
- A session's agent runs on the daemon's own stdio
  (`acp_runtime.rs::an_acp_author_runs_on_daemon_stdio`).
- A session row and its events round-trip through the store, and a live one
  is found as live (`store.rs::sessions_and_events_round_trip`).
- Restarting a session reopens the same row
  (`store.rs::restarting_a_session_reopens_the_same_row`), and every launch is
  dated (`::every_launch_of_a_session_is_dated`).
- Every launch reports under an id of its own, carried by the agent's MCP
  server (`resume.rs::every_launch_of_a_session_reports_under_a_new_id`), and
  a report from a launch the row has moved past changes nothing
  (`events.rs::an_event_from_a_launch_the_session_has_moved_past_changes_nothing`).
- A relaunch announces the session as updated
  (`resume.rs::a_relaunch_announces_the_session_as_updated`).
- The author and the reviewer reuse one session across reviews
  (`resume.rs::resuming_the_author_reuses_its_session_across_reviews`,
  `::a_reviewer_reuses_its_session_across_reviews`).
- Killing a session kills its agent process
  (`acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`).
- Reviving a session revives it in place
  (`resume.rs::reviving_a_session_revives_it_in_place`); a session without an
  agent id is not revived (`::a_session_without_an_agent_id_is_not_revived`),
  and neither is a session of a finished goal
  (`::a_session_of_a_finished_goal_is_not_revived`).
- The console stream gives the snapshot, then deltas
  (`acp_console.rs::the_console_stream_gives_the_snapshot_then_deltas`).
- A permission request and its reply appear in the console stream
  (`acp_console.rs::a_permission_request_appears_in_the_console_stream`).
- Posted input reaches the agent as a prompt, and input posted while a turn
  runs is queued and sent once it ends
  (`acp_console.rs::posted_input_reaches_the_agent_and_queues_behind_a_running_turn`).
- A console answer selects the option of a pending question, and takes the
  session's flag down
  (`acp_console.rs::ask_raises_attention_and_a_console_answer_unblocks_the_turn`).
- A lagged console client is told to resync, and the connection closes
  (`acp_console.rs::a_lagged_console_client_gets_a_resync_and_the_stream_ends`).
- The stored and the live events go out in id order, on the console stream
  and the terminal socket alike
  (`http/console.rs::a_stored_whole_does_not_pass_the_live_chunk_that_preceded_it`),
  and the merge keeps other sessions' events and the snapshot's last event out
  (`::the_merge_keeps_other_sessions_and_the_snapshots_last_event_out`). The
  ids are published in the order they are taken: a stored event holds the
  store's event lock from its id to its publication
  (`store.rs::an_event_that_waits_for_the_event_lock_takes_its_id_after_the_one_that_held_it`,
  `::events_written_at_once_are_published_in_id_order`), and a live event
  takes its id and goes out under the same lock
  (`acp.rs::a_live_event_that_waits_for_the_event_lock_takes_its_id_after_the_one_that_held_it`).
  A stored event reaches the bus only after the bus has loaded what its
  event carries, so a live event can still reach the merge first; the store
  holds every stored event with a lower id by then, and the merge reads
  those it has not sent before the live event goes out, and drops their
  later copies off the bus
  (`http/console.rs::a_live_event_waits_for_the_stored_events_below_it_that_the_bus_has_not_delivered`).
  A merge that cannot read the store holds the live event back and says
  resync
  (`::a_merge_that_cannot_read_the_store_says_resync_instead_of_sending_a_live_event`),
  and one whose live channel dropped events while the console opened says
  resync before anything else
  (`::a_live_channel_that_lagged_while_the_console_opened_is_told_to_resync_first`);
  `GET /console` opens again under such a lag and refuses when it keeps
  lagging
  (`::the_snapshot_get_answers_is_opened_again_under_a_lag_and_refused_when_it_keeps_lagging`).
- A stream opened mid-turn reads the text so far in its snapshot
  (`acp_console.rs::a_stream_opened_mid_turn_gets_the_text_so_far_in_its_snapshot`),
  where the text sits by id among the stored events
  (`http/console.rs::the_text_so_far_sits_by_id_among_the_stored_events`),
  a live chunk sent between the two reads joins the snapshot in its place
  and is not sent again
  (`::a_live_chunk_sent_between_the_two_reads_joins_the_snapshot_in_its_place`),
  a live event above the snapshot's last stored event follows it once
  (`::a_live_event_above_the_snapshots_last_stored_event_follows_it_once`),
  and the chunks arrive live before the turn ends
  (`::a_console_stream_client_sees_message_chunks_before_the_turn_ends`).
- Cancel ends the running turn as `cancelled`
  (`acp_console.rs::cancelling_a_running_turn_ends_it_as_cancelled`), is
  refused between turns (`::cancel_with_no_turn_running_is_refused`), and is
  in the OpenAPI document (`::the_cancel_endpoint_is_in_the_openapi_document`).
- The terminal socket serves bytes that draw the transcript and the status
  line at the size the client sent
  (`acp_terminal.rs::the_terminal_draws_the_transcript_and_the_status_line_at_the_client_size`),
  typed keys and Enter reach the agent as a prompt
  (`::typed_keys_and_enter_reach_the_stub_agent_as_a_prompt`), a key answers
  a pending permission question
  (`::a_key_answers_a_pending_permission_question`), closing the socket
  leaves the session alive and the session ending closes the socket
  (`::closing_the_socket_leaves_the_session_alive_and_the_session_ending_closes_it`),
  a socket opened after the session ended gets the transcript and closes
  (`::a_socket_opened_after_the_session_ended_gets_the_transcript_and_closes`;
  the loop leaves on such a snapshot:
  `ariadne-console/tui.rs::a_snapshot_that_holds_the_session_end_leaves_the_console`),
  Ctrl-C twice and Ctrl-D close it with the session alive
  (`::ctrl_c_twice_or_ctrl_d_closes_the_socket_and_the_session_stays_alive`),
  a resize redraws at the new size (`::a_resize_redraws_at_the_new_size`),
  an unknown session is refused before the upgrade
  (`::a_terminal_for_an_unknown_session_is_refused_before_the_upgrade`), and
  the endpoint is in the OpenAPI document
  (`::the_terminal_endpoint_is_in_the_openapi_document`).
- The ANSI backend draws what the test backend draws
  (`ariadne-console/ansi.rs::the_bytes_draw_what_the_test_backend_draws`),
  keeps a bold cell bold when the dim beside it ends
  (`::a_cell_keeps_its_bold_when_the_dim_beside_it_ends`),
  knows the cursor and the size without a query
  (`::the_cursor_and_the_size_are_known_without_a_query`), pushes inserted
  lines into the emulator's scrollback
  (`::an_inline_viewport_pushes_inserted_lines_into_the_scrollback`), and
  redraws the viewport at a new size
  (`::a_resize_redraws_the_viewport_at_the_new_size`).
- The CLI console renders a transcript and delivers an input line
  (`console.rs::a_console_renders_a_stub_agent_transcript_and_delivers_an_input_line`),
  and renders a permission question and delivers the selected answer
  (`::a_permission_question_renders_and_delivers_the_selected_answer`). Both
  are the plain protocol a redirected console keeps
  (`console/tui.rs::only_a_terminal_on_both_ends_gets_the_inline_console`).
- The inline pane renders the prompt, the agent's markdown, a tool result
  folded to its last lines with its trailing blank lines trimmed, and the
  status line
  (`ariadne-console/tui.rs::a_transcript_renders_the_prompt_the_markdown_the_tool_call_and_the_status_line`),
  and markdown keeps a heading, a code block and a list apart
  (`ariadne-console/markdown.rs::a_heading_a_code_block_and_a_list_each_keep_their_own_style`,
  `::a_paragraph_wraps_at_the_width_it_is_drawn_at`).
- The status line follows the session's status from its events
  (`ariadne-console/tui.rs::the_status_line_follows_the_sessions_status_from_its_events`)
  and is not revived off an end by the events replayed under it
  (`::a_header_that_says_exited_or_failed_is_not_revived_by_the_events_replayed`),
  while a live row resumed after an earlier end follows its new launch
  (`::a_live_session_resumed_after_its_end_follows_its_new_launch_not_the_old_end`),
  counts the running turn and stops between turns
  (`::the_status_line_counts_the_running_turn_and_stops_between_turns`),
  a turn already running at attach counts from its prompt
  (`::an_attach_during_a_turn_counts_from_the_prompt_that_began_it`), a
  reconnect's replay keeps the clock of the turn still running
  (`::a_reconnect_keeps_the_clock_of_the_turn_still_running`) and gives a
  turn begun while the stream was down its own clock
  (`::a_turn_begun_while_the_stream_was_down_counts_from_its_own_prompt`),
  and a resize
  redraws the pane at the new size
  (`::a_resize_redraws_the_viewport_at_the_new_size`).
- A line of wide characters wraps at the display width, in a block
  (`ariadne-console/tui.rs::a_line_of_wide_characters_wraps_at_the_display_width`)
  and in markdown
  (`ariadne-console/markdown.rs::a_paragraph_of_wide_characters_wraps_at_the_display_width`);
  the cursor sits after the columns a wide character draws on
  (`ariadne-console/tui.rs::the_cursor_sits_after_the_columns_a_wide_character_draws_on`),
  an emoji sequence measured as it is drawn
  (`::the_cursor_sits_after_an_emoji_sequence_as_it_is_drawn`); and a cut
  keeps an emoji sequence whole, in a call's head
  (`::a_head_is_cut_between_whole_emoji_sequences`) and in a code line
  (`ariadne-console/markdown.rs::a_code_line_is_cut_between_whole_emoji_sequences`).
- A tool call's head is a glyph per kind and what the call is about — the
  command, the path and line, the pattern and path, the URL — never raw JSON
  (`ariadne-console/tui.rs::each_kind_of_call_draws_its_glyph_and_what_it_is_about`);
  a completed call draws its duration
  (`::a_completed_call_draws_its_duration`); and updates of one call draw one
  block (`::updates_of_one_call_draw_one_block`), because they fold into the
  open call and the last dates its end
  (`ariadne-console/transcript.rs::updates_of_one_call_fold_into_it_and_the_last_dates_its_end`);
  a call a question asks about reaches the scrollback as it ended, not as
  pending
  (`ariadne-console/tui.rs::a_call_a_question_asks_about_reaches_the_scrollback_as_it_ended_not_as_pending`).
- A diff draws a file header, coloured lines and a fold count past the limit
  (`ariadne-console/tui.rs::a_diff_draws_its_file_header_its_lines_coloured_and_a_fold_count`),
  and an old text and a new text fold to hunks with context rather than every
  old line and then every new one
  (`ariadne-console/transcript.rs::an_old_and_a_new_text_fold_to_hunks_with_context`). A
  patch without file headers takes them from the entry's `path`
  (`ariadne-console/transcript.rs::a_patch_without_file_headers_takes_them_from_the_entry_path`).
- A permission question draws the call's head and its command or its diff
  above the options
  (`ariadne-console/tui.rs::a_permission_question_draws_the_call_above_its_options`),
  and while it waits the picker is on the screen whatever came after it and
  however long its diff
  (`::a_pending_picker_is_on_the_screen_whatever_came_after_it_and_however_long_its_diff`).
- A tab in a tool's output takes the columns to the next tab stop
  (`ariadne-console/tui.rs::a_tab_in_a_tool_output_takes_the_columns_to_the_next_tab_stop`),
  and a call whose raw output is a structure draws its content text
  (`ariadne-console/transcript.rs::a_structured_raw_output_gives_way_to_the_content_text`).
- A prompt draws its text alone, never the system prompt
  (`ariadne-console/tui.rs::a_prompt_draws_its_text_alone_and_never_the_system_prompt`);
  an event carrying only the whole prompt draws none of it
  (`::a_prompt_carrying_only_the_whole_prompt_draws_no_system_prompt`); a
  confirmation without `text` keeps what was typed
  (`::a_typed_line_confirmed_without_its_text_keeps_what_was_typed`) while
  an older daemon's own prompt takes no pending prompt's place
  (`::an_older_daemons_own_prompt_does_not_take_a_pending_prompts_place`); and a
  daemon-sourced prompt draws under its own marker
  (`::a_daemon_sourced_prompt_draws_under_its_own_marker`).
- Where the cursor position cannot be read, the pane opens from the bottom
  row
  (`ariadne-console/tui.rs::the_console_opens_at_the_bottom_when_the_cursor_position_cannot_be_read`)
  and a finished block still reaches the scrollback
  (`::a_finished_block_reaches_the_scrollback_when_the_cursor_position_cannot_be_read`).
  A terminal that answered at the open and answers nothing after — the
  terminal once the key stream reads it — is never asked again, and a
  finished block still reaches the scrollback
  (`::a_finished_block_reaches_the_scrollback_when_only_the_first_cursor_query_is_answered`).
- Streamed chunks append to the block already open
  (`ariadne-console/tui.rs::streamed_chunks_append_to_the_agent_block_that_is_already_open`),
  a chunk that arrives after the whole of its turn is not drawn again
  (`::a_chunk_that_arrives_after_the_whole_of_its_turn_is_not_drawn_again`),
  and text after a tool call is a block of its own that the stored whole does
  not repeat (`::agent_text_after_a_tool_call_is_a_block_of_its_own`).
- A permission question is a picker the arrows move
  (`ariadne-console/tui.rs::a_permission_question_renders_as_a_picker_the_arrows_move`),
  and Enter posts the option it is on
  (`::enter_posts_the_permission_option_the_picker_is_on`).
- A typed line is posted and its pending prompt shows at once
  (`ariadne-console/tui.rs::a_pending_prompt_is_on_the_screen_before_the_daemon_confirms_it`)
  and is replaced by the confirmed one
  (`::a_typed_line_is_posted_and_its_pending_prompt_is_replaced_by_the_confirmed_one`);
  Shift+Enter and Alt+Enter add a line instead
  (`::shift_enter_and_alt_enter_add_a_line_instead_of_sending`), and a line
  longer than the box scrolls under the cursor
  (`::a_line_longer_than_the_input_box_scrolls_under_the_cursor`). Two
  prompts posted before the first is confirmed stay apart
  (`::a_second_prompt_typed_before_the_first_is_confirmed_keeps_both_apart`),
  and what a running turn says is drawn above a prompt typed while it ran
  (`::what_a_running_turn_says_is_drawn_above_a_prompt_typed_while_it_ran`).
  A refused prompt is said on the transcript and does not close the console
  (`::a_refused_prompt_is_said_on_the_transcript_and_does_not_close_the_console`).
- A pasted text with two line breaks is one prompt with two line breaks, and
  sends nothing until Enter
  (`ariadne-console/tui.rs::a_pasted_text_is_one_prompt_with_its_line_breaks_and_sends_nothing_until_enter`);
  it goes in at the cursor, a carriage return being a line break
  (`::a_paste_goes_in_at_the_cursor_and_a_carriage_return_is_a_line_break`).
- The line-editing keys do what the shell's do: Ctrl-A
  (`ariadne-console/tui.rs::ctrl_a_moves_to_the_line_start`), Ctrl-E
  (`::ctrl_e_moves_to_the_line_end`), Ctrl-U
  (`::ctrl_u_deletes_to_the_line_start`), Ctrl-K
  (`::ctrl_k_deletes_to_the_line_end`), Ctrl-W
  (`::ctrl_w_deletes_the_word_before_the_cursor`) and the Alt arrows
  (`::alt_left_and_alt_right_move_by_word`).
- Escape cancels the running turn
  (`ariadne-console/tui.rs::escape_during_a_running_turn_cancels_it`), one
  Ctrl-C keeps the console and the second leaves it
  (`::one_ctrl_c_keeps_the_console_and_the_second_leaves_it`), and the pane
  opened from the bottom row runs the loop and closes like the inline one
  (`::the_console_runs_and_closes_on_the_fallback_viewport`). The terminal is
  given back on every way out
  (`console/tui.rs::the_terminal_is_given_back_on_the_normal_path_on_an_error_and_on_ctrl_c`),
  with bracketed paste off
  (`::the_terminal_is_given_back_with_bracketed_paste_off`).
- A dropped stream says so
  (`ariadne-console/tui.rs::a_dropped_stream_says_reconnecting_on_the_status_line`),
  until the next snapshot says it is back
  (`::a_dropped_frame_says_reconnecting_until_the_next_snapshot`), and its
  fresh snapshot is not printed twice
  (`::a_reconnect_redraws_the_fresh_snapshot_without_repeating_the_scrollback`).
  The loop reads any source and writes any sink: every pane test above
  drives it from an in-memory one, with no server. The CLI's source reads
  the daemon's stream as frames and dials it again when it drops
  (`console/tui.rs::the_stream_is_read_as_frames_and_dialled_again_when_it_drops`),
  ends with the error when the first dial is refused
  (`::a_first_dial_the_daemon_refuses_ends_the_stream_with_the_error`), and
  its sink posts input and cancel and says a refusal
  (`::the_sink_posts_input_and_cancel_and_says_a_refusal`).
- A readable transcript folds tool and permission pairs, keeps full text and
  renders plain output without colour
  (`transcript.rs::a_transcript_renders_one_full_block_per_item`,
  `::no_color_gives_plain_transcript_text`).
- Tool diffs retain diff colouring
  (`transcript.rs::a_tool_call_diff_uses_diff_colouring`).
- Transcript snapshots apply tail, time and kind filters
  (`console.rs::transcript_snapshot_filters_apply_to_folded_items`).
- `ariadne session logs` keeps JSON events unchanged and follows the console
  stream (`console.rs::a_transcript_log_uses_its_snapshot_for_table_and_json_output`,
  `::a_followed_log_uses_the_console_event_stream`). Chunks stream under one
  block header (`::followed_chunks_stream_text_under_one_block_header`).

## Known gap

- The launcher refuses a second live session on one seat, and no test pins
  that refusal on its own.
- No test pins the `409` a finished session gives to console input.

## Sources

`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/http/sessions.rs`,
`crates/ariadne-daemon/src/http/console.rs`,
`crates/ariadne-daemon/src/http/terminal.rs`,
`crates/ariadne-daemon/src/acp.rs`,
`crates/ariadne-cli/src/commands/attach.rs`,
`crates/ariadne-cli/src/commands/console.rs`,
`crates/ariadne-cli/src/commands/console/tui.rs`,
`crates/ariadne-cli/src/commands/transcript.rs`,
`crates/ariadne-console/src/ansi.rs`,
`crates/ariadne-console/src/markdown.rs`,
`crates/ariadne-console/src/transcript.rs`,
`crates/ariadne-console/src/tui.rs`.
