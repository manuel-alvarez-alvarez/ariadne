---
id: sessions-terminals-and-logs
status: current
updated: 2026-09-12
areas: [daemon, store, cli]
commits: [e4816cf6, 39937143, a69b953f]
tests:
  - crates/ariadne-daemon/tests/resume.rs
  - crates/ariadne-daemon/tests/acp_console.rs
  - crates/ariadne-daemon/tests/acp_runtime.rs
  - crates/ariadne-daemon/tests/events.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/console.rs
  - crates/ariadne-cli/src/commands/console/tui.rs
  - crates/ariadne-cli/src/commands/console/markdown.rs
---

# Sessions and the console

An agent session is a row the daemon keeps for one agent on one seat, and the
agent behind it is a daemon-owned ACP child process (021). This is what a
session holds, what may be done to it, and how a client reads it and speaks
to it: through the session's console.

## Scope

In: the session row and its statuses, launches and relaunches of one row,
kill and resume, the console — its transcript, its live stream and posted
input — and how the CLI reaches the console.

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
    While a turn runs, the snapshot ends on the text so far: one
    `agent_thought_chunk` and one `agent_message_chunk`, each only where
    there is text, after the stored events.
14. The console stream (`GET /v1/sessions/{id}/console/stream`) opens with
    that snapshot, then sends each later event as it is recorded, and each
    live-only event as the runtime streams it (021): message and thought
    chunks, and tool call progress. A client that connects mid-turn reads
    the text so far in its snapshot and every later chunk on the stream,
    none of them twice.
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
21. The CLI reaches a session through its console. `ariadne attach` renders
    the transcript, follows the stream and posts what is typed as input.
    `ariadne session logs` and `ariadne task logs` print the snapshot as typed
    transcript blocks. Paired tool and permission events form one block;
    agent text stays whole, and diffs retain their line colouring. `--tail`,
    `--since` and `--kind` filter the snapshot. With `--follow`, chunks stream
    under one item header and `--kind` also filters later events. JSON output
    keeps each event object unchanged.
22. `ariadne attach` on a terminal is an inline pane, never the alternate
    screen. A finished block goes into the terminal's own buffer above the
    pane, so it stays in the scrollback; the pane holds the block still being
    written, a status line and the input box. The status line names the seat,
    the model and the session's status, and turns a spinner with "thinking" or
    "running &lt;tool&gt;" while a turn runs. The pane opens from the cursor,
    which the terminal is asked for; a terminal that does not answer within
    crossterm's timeout gets the same pane opened from the bottom row instead,
    on a backend that answers every later cursor query itself. There is no
    alternate-screen fallback.
23. The pane renders each block as it arrives: a prompt as `> text`, holding
    the event's `text` alone — never the whole `prompt` with the system
    prompt ahead of it (021), nor the summary, one line cut short; an event
    carrying no `text` says the text was not recorded rather than draw the
    whole. A prompt the daemon sent (`source: daemon`: a briefing,
    a nudge, a message) draws under its own marker and label, `» daemon`,
    with its text beneath, so it reads apart from what was typed. Agent text
    is markdown chunk by chunk under one marker, a thought dimmed and folded,
    a tool call one line with a status glyph, its name and its input with its
    output folded under it, a diff coloured, a plan a checklist, and a
    permission question a picker.
24. A chunk continues the block last written, and starts a block of its own
    where anything else came between: a turn that speaks around a tool call
    reads as two blocks with the call between them. The whole text the daemon
    stores at the end of that turn (021) is every chunk of it joined, so it
    closes the blocks the chunks opened and repeats none of them. A turn that
    streamed nothing renders that stored text as its one block.
25. Enter posts the input box to console input, Shift+Enter and Alt+Enter add
    a line to it, and each prompt typed shows at once and is replaced by its
    own `user_prompt_submit`, in the order they were posted. An older
    daemon's (021) prompt event carries neither `text` nor `source`: it takes
    the oldest pending prompt's place where its whole `prompt` ends in a
    blank line and then the typed text — the whole is the system prompt, a
    blank line and the text, so a daemon prompt that merely ends in the same
    words does not match — keeping what was typed, and is a prompt of its
    own otherwise. On a permission
    question the arrows and the number keys move the pick and Enter posts the
    option's id. A post the daemon refuses is said on the transcript, and the
    console stays open.
26. Escape during a running turn posts to console cancel. Ctrl-C twice, or
    Ctrl-D, leaves the console, and the session stays alive. Every way out
    puts the terminal back: raw mode off and the cursor shown.
27. A dropped stream says "reconnecting" and is dialled again on the backoff
    every other follow uses. The fresh snapshot redraws what was open and does
    not repeat what is already in the scrollback.
28. With stdin or stdout redirected there is no pane. `ariadne attach` is then
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
- A stream opened mid-turn reads the text so far in its snapshot
  (`acp_console.rs::a_stream_opened_mid_turn_gets_the_text_so_far_in_its_snapshot`),
  and the chunks arrive live before the turn ends
  (`::a_console_stream_client_sees_message_chunks_before_the_turn_ends`).
- Cancel ends the running turn as `cancelled`
  (`acp_console.rs::cancelling_a_running_turn_ends_it_as_cancelled`), is
  refused between turns (`::cancel_with_no_turn_running_is_refused`), and is
  in the OpenAPI document (`::the_cancel_endpoint_is_in_the_openapi_document`).
- The CLI console renders a transcript and delivers an input line
  (`console.rs::a_console_renders_a_stub_agent_transcript_and_delivers_an_input_line`),
  and renders a permission question and delivers the selected answer
  (`::a_permission_question_renders_and_delivers_the_selected_answer`). Both
  are the plain protocol a redirected console keeps
  (`console/tui.rs::only_a_terminal_on_both_ends_gets_the_inline_console`).
- The inline pane renders the prompt, the agent's markdown, a folded tool
  result and the status line
  (`console/tui.rs::a_transcript_renders_the_prompt_the_markdown_the_tool_call_and_the_status_line`),
  and markdown keeps a heading, a code block and a list apart
  (`console/markdown.rs::a_heading_a_code_block_and_a_list_each_keep_their_own_style`,
  `::a_paragraph_wraps_at_the_width_it_is_drawn_at`).
- A prompt draws its text alone, never the system prompt
  (`console/tui.rs::a_prompt_draws_its_text_alone_and_never_the_system_prompt`);
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
  (`console/tui.rs::the_console_opens_at_the_bottom_when_the_cursor_position_cannot_be_read`)
  and a finished block still reaches the scrollback
  (`::a_finished_block_reaches_the_scrollback_when_the_cursor_position_cannot_be_read`).
- Streamed chunks append to the block already open
  (`console/tui.rs::streamed_chunks_append_to_the_agent_block_that_is_already_open`),
  and text after a tool call is a block of its own that the stored whole does
  not repeat (`::agent_text_after_a_tool_call_is_a_block_of_its_own`).
- A permission question is a picker the arrows move
  (`console/tui.rs::a_permission_question_renders_as_a_picker_the_arrows_move`),
  and Enter posts the option it is on
  (`::enter_posts_the_permission_option_the_picker_is_on`).
- A typed line is posted and its pending prompt shows at once
  (`console/tui.rs::a_pending_prompt_is_on_the_screen_before_the_daemon_confirms_it`)
  and is replaced by the confirmed one
  (`::a_typed_line_is_posted_and_its_pending_prompt_is_replaced_by_the_confirmed_one`);
  Shift+Enter and Alt+Enter add a line instead
  (`::shift_enter_and_alt_enter_add_a_line_instead_of_sending`), and a line
  longer than the box scrolls under the cursor
  (`::a_line_longer_than_the_input_box_scrolls_under_the_cursor`). Two
  prompts posted before the first is confirmed stay apart
  (`::a_second_prompt_typed_before_the_first_is_confirmed_keeps_both_apart`).
  A refused prompt is said on the transcript and does not close the console
  (`::a_refused_prompt_is_said_on_the_transcript_and_does_not_close_the_console`).
- Escape cancels the running turn
  (`console/tui.rs::escape_during_a_running_turn_cancels_it`), one Ctrl-C
  keeps the console and the second leaves it
  (`::one_ctrl_c_keeps_the_console_and_the_second_leaves_it`), and the
  terminal is given back on every way out, the pane opened from the bottom
  row included
  (`::the_terminal_is_given_back_on_the_normal_path_on_an_error_and_on_ctrl_c`).
- A dropped stream says so
  (`console/tui.rs::a_dropped_stream_says_reconnecting_on_the_status_line`) and
  its fresh snapshot is not printed twice
  (`::a_reconnect_redraws_the_fresh_snapshot_without_repeating_the_scrollback`).
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
`crates/ariadne-daemon/src/acp.rs`,
`crates/ariadne-cli/src/commands/attach.rs`,
`crates/ariadne-cli/src/commands/console.rs`,
`crates/ariadne-cli/src/commands/console/tui.rs`,
`crates/ariadne-cli/src/commands/console/markdown.rs`,
`crates/ariadne-cli/src/commands/transcript.rs`.
