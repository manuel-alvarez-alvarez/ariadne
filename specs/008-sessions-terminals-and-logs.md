---
id: sessions-terminals-and-logs
status: current
updated: 2026-11-27
areas: [daemon, cli, ui]
commits: [e4816cf6, 39937143, a69b953f, 7880c022, 29e6d84e, 1b09ac10, a4d7da95]
tests:
  - crates/ariadne-store/tests/session_place.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-daemon/tests/it/acp_terminal.rs
  - crates/ariadne-daemon/tests/it/resume.rs
  - crates/ariadne-daemon/tests/it/switch.rs
  - crates/ariadne-cli/src/commands/transcript.rs
  - crates/ariadne-console/src/trajectory/print.rs
  - crates/ariadne-console/src/app/tests.rs
  - crates/ariadne-console/src/app/history.rs
  - crates/ariadne-daemon/tests/it/far_tasks/source_references.rs
  - crates/ariadne-cli/src/commands/console/app.rs
  - crates/ariadne-cli/src/commands/console/terminal.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - crates/ariadne-client/src/lib.rs
---

# Sessions, terminals and logs

How Ariadne keeps agent sessions and lets clients read, control, and switch
them.

## Scope

In: session rows, launches, kill and resume, the console input and cancel routes, terminal
WebSockets, CLI attach, the full-screen console, and session switches.

Out: ACP process ownership (021), task transitions (001), and prompt wording
(006).

## Behavior

1. A staffed session belongs to a goal and seat, and a column session also belongs to its task.
2. A session stores its worktree, model, optional effort, agent session id, and optional place.
3. A session is starting, running, idle, exited, or failed.
4. A task reuses one session for each column agent, and a goal reuses one orchestrator session.
5. Each launch has a new launch id, which is stored before the agent starts.
6. A stale launch report cannot update a later launch of the same session.
7. Killing a live session kills its process and leaves the row resumable.
8. Resume revives the same row and conversation, using the repository checkout when its worktree is absent.
   Place selection and old-process cleanup follow 021 rule 17.
9. A session without an agent conversation cannot resume, and a cancelled goal session cannot revive.
10. `GET /v1/sessions/{id}/console` and `GET /v1/sessions/{id}/console/stream` are gone: each answers `404`, also for a session on a remote host, which the source answers without asking the worker, and the OpenAPI document lists neither.
11. A session's turns are read through the trajectory (040), and its stored events through `GET /v1/events` (012).
12. `POST /v1/sessions/{id}/console/input` sends or queues one prompt and answers a pending permission request.
13. A finished or unavailable session refuses console input with `409`.
14. `POST /v1/sessions/{id}/console/cancel` cancels a running turn and refuses a session without a running turn with `409`.
15. `GET /v1/sessions/{id}/console/terminal` upgrades a known session to the terminal WebSocket.
    It sends the full-screen console of rules 39 to 58, as terminal bytes: the alternate screen, the mouse and bracketed paste, then the screen.
    It sends nothing before the client's first `resize`.
    A session on a remote host serves the route through the source's forwarder, with the frames of both ways unchanged.
    The host of this route reads no stored model, no typed detail and no subagent steps.
    So its status row names the model only where the agent sends a model option (rule 57).
    An open tool call shows the text of its step and the reason, in place of its detail (rule 51).
    A subagent opens only to the live steps of its lane that the view holds (rule 53).
16. Closing the terminal socket leaves the session alive, and session completion closes its sockets.
    As the console ends, the socket gives the screen back and writes the turns it showed, folded, to the main screen (rule 50).
17. The terminal accepts `resize`, `key`, `paste` and `mouse` messages.
    A `resize` draws the whole screen again at the client size (rule 47).
    A `mouse` message has a `kind` and the cell, `col` and `row`, counted from zero.
    `wheel_up` and `wheel_down` are one step of the wheel, and `click` is the left button.
    The console obeys each as rule 46 says.
18. See [015](015-desktop-app.md) for the desktop console, which reads the trajectory stream and posts to the input and cancel routes rather than the terminal WebSocket.
19 to 22. Ended with the inline attach pane, which is gone. The console of rules 39 to 58 is the one `attach` opens.
23. A session switch records the old session and starts the replacement with its handoff.
24. A same-agent switch keeps the conversation and applies the new model and effort.
25. A cross-agent switch starts a new conversation and leaves the old session exited.
26. A cancelled-goal session, a model turned off, or an already switched session refuses switching.
27. See [033](033-telemetry.md) for the daemon's own log: the telemetry store it is read from, and its snapshot and stream routes.
28 to 35. Ended with the inline attach pane. Rules 43 to 58 hold the outcome of each in the full-screen console.
36. `POST /v1/sessions/{id}/console/cancel` takes an optional body, `{"subagent_id"}`. Named, it cancels that subagent's turn (021 rule 16) instead of the session's own, which is left as it is; the body omitted, or `subagent_id` omitted, cancels the session's own turn as before. It refuses with `409` a subagent that is unknown, has already ended, or was never said to be cancellable.
37. Ended with the inline attach pane. Rule 48 shows an open question above the input.
38. An agent form uses one question per step above the input.
    The header shows its title, or "question", its position when multiple questions exist, and answer marks.
    The question text precedes numbered choices. Descriptions use the dim style.
    A case-insensitive "(Recommended)" suffix becomes an accent "recommended" tag.
    - An Other companion is a plain string field after its choice field, with key `<choice key>_custom`.
      Claude sends `question_N` and `question_N_custom`. Codex and OpenCode send no companion.
      A question combines that pair, or contains one field.
    - Picks remain in `content[key]`. Typed answers remain in `content[key_custom]`.
      A typed single answer sends only its companion key.
      A typed multi-select answer with no checked choices also sends only its companion key.
      The answer endpoint remains `POST /v1/sessions/{id}/questions/{question_id}/answer`.
    - The companion appears as the last choice, "Type your own answer…".
      Enter takes text from the input. The companion has no separate step.
    - Up and Down move between choices. Single-select numbers 1–9 or Enter answer and advance.
      Multi-select Space checks a choice, and Enter advances.
      The last step sends the form only when every required question has an answer.
      Otherwise, it returns to the first missing required question and shows "<title> needs an answer."
      A companion answer satisfies its required choice question. Tab and Left/Right move between questions only while the input box is empty; with text in it they belong to the box.
      Esc declines.
    - Other field types retain one step per field and their existing validation.
      Empty forms accept Enter. Permission questions take keys first.
    - Closed forms show compact summaries in the console and transcript output.
      Each question shows "<field title, else the message> → <answer>", with custom text merged.
      Declined, withdrawn and cancelled forms show one line.
      Followed logs draw the close when it arrives.

### The full-screen console

`ariadne attach` opens it on a terminal, with no flag.
Its code is `crates/ariadne-console/src/app/`.
The inline pane of rules 19 to 22, 28 to 35 and 37 is gone with its code: `attach` opens only this console.

39. On a terminal, `attach` takes the alternate screen, raw mode, the mouse and bracketed paste.
    The console gives the terminal back on each way out: a leave, an error, a panic and Ctrl-C.
    Without a terminal on both ends, `attach` prints the line protocol.
    `attach` has no `--full-screen` flag.
40. The console reads the trajectory stream (040) and draws the client view of the trajectory.
    A drop, a `resync`, or an update that skips a revision shows `reconnecting` and dials the stream again.
    The next snapshot replaces the view. Thus a reconnect shows no item two times.
41. The transcript shows each turn as its prompt, one activity line and its answer.
    The activity line has the fold mark, the state mark, the counts of the turn and its duration.
    It prints no state word for a completed turn: the mark says it.
    The answer is markdown and appears while the turn runs. A daemon prompt shows its first line only.
    A folded running turn shows its last three steps, live.
    The latest message supplies the running answer as text arrives. Its step row keeps the status without repeating the answer.
42. An open turn shows a row for each step under its activity line.
    A row has the status mark, the kind, the subject and the duration.
    The kind of a tool call is the icon of its kind, and its row prints no kind word (rule 59).
    The console reads the complete steps from the steps route (040), including when an open turn finishes.
43. The screen has two zones: the transcript and the input. The input zone has the keys at the start.
    Tab gives the keys to the transcript, with the cursor on the newest turn.
    In the transcript, Tab, Escape or a typed character gives the keys back to the input.
    A typed character goes into the input box.
44. In the input zone, Enter sends the box. Up and Down move through the rows of the box, then through the prompt history.
    PgUp and PgDn scroll the transcript. Ctrl-O opens or closes the newest turn.
    Escape cancels the running turn.
    Tab completes the pick while the suggestion list is open, and moves between the questions of a form.
45. In the transcript zone, Up, Down, `j` and `k` move the cursor between turns and the steps of open turns.
    The line of the item at the cursor is reversed.
    Enter and Right open the item in place. An open step shows its detail under its row (rule 51).
    Left closes an open item, or moves the cursor from a step to its turn.
    Home and End move the cursor to the first and the newest turn.
    `y` copies the item at the cursor as `ariadne session logs` prints it. The CLI sends it to the clipboard with OSC 52.
46. The wheel scrolls the transcript three lines in both zones. A click on an item opens or closes it, and moves the cursor to it.
    A transcript scrolled to its end follows the new lines.
47. A resize draws the whole screen again from the view, at the new size. No line is lost.
    The screen on an ANSI terminal is the screen that the test backend draws.
48. A pending permission and an open question show above the input.
    The active question and selected answer stay visible after resize when there is room for them.
    The permission takes the keys of rule 12: Up, Down and a number pick, and Enter sends the option.
    Failed question reads retry at most three times per snapshot; a reconnect permits another attempt.
    The question is the form of rule 38. The console reads its form from `GET /v1/sessions/{id}/questions`.
49. The input zone holds the input box, the prompt history, the slash commands, the queued prompts and Ctrl-C.
    On a truncated trajectory, the host reads the prompt history through the pages of older turns of the trajectory (040), at most four pages of 50 turns.
    The terminal of rule 15 does the same.
    The slash commands are the commands of the session in the trajectory.
    A sent prompt shows `queued` until a new console turn holds its text.
    A refused prompt leaves the queue, and the console shows the refusal under the transcript.
    One Ctrl-C arms the leave and the bottom row says so (rule 60). A second Ctrl-C leaves. Ctrl-D leaves at once.
50. On leaving, the console prints the turns that this attach showed, folded, as `ariadne session logs` prints them.
    It prints the newest 20 at most, then a hint to `ariadne session logs <id> --steps`.
    A session that ended before the attach leaves after its snapshot.
51. An open tool call step reads its detail from the detail route (040) and shows it by its kind.
    A command shows its command, its working directory, its output and its exit code.
    An edit shows each file as a coloured diff.
    A read shows the path and the line, then the file text coloured by the grammar of its language.
    A search and a fetch show what they looked for and what they found. Any other call shows its content.
    An output keeps its last 12 lines. A diff, a file and raw JSON keep their first 24 lines.
    Each fold shows a count of the lines it left out. A second Enter on the open step shows it whole.
    A change of `detail_rev` reads the detail again.
    A host that cannot read the detail shows the text of the step and the reason.
    Any other open step shows its whole text under its row.
52. In the transcript zone, `r` shows the raw input and the raw output of the tool call at the cursor, as JSON.
    The console reads them with `raw=true`. A second `r` shows the detail again.
53. While the session has a subagent, the top row is a tab bar: `main`, then one tab for each subagent of the trajectory's `subagents` list, in start order.
    A session with no subagent shows no bar, and the transcript uses the row.
    A subagent tab shows the state mark of the subagent and its name. The active tab is in brackets and reversed, so it is clear without colour.
    A narrow bar cuts the names to four columns and keeps the marks. Then it shows the tabs round the active one, with `‹` and `›` where tabs are past an edge.
    From five columns, the active tab cuts its own name further, so its brackets, its state mark and the edge marks fit whole.
    In `main`, a subagent is one summary row: the state mark, the subagent icon `↳`, the name, the step count and the time. It does not open in place.
    Enter or a click on a summary row opens the tab of that subagent.
    A subagent tab shows its task, then its steps with the rows of an open turn (rules 42, 51 and 59). Its steps do not name their lane.
    The console reads them with `subagent=<id>` (040, rule 44). While the turn runs, the live steps of its lane join them.
    Ctrl-N moves to the next tab and Ctrl-P to the tab before; the ends wrap. Esc in a subagent tab returns to `main`. Tab still moves between the zones.
    Each tab keeps its own scroll position and cursor row. The input sends to the session from every tab.
54. While a plan of entries has an entry that is not completed, one plan line shows above the input.
    The line has the count of the completed entries and the entry in progress, else the next one.
    Ctrl-T, or a click on the line, opens it to the checklist and closes it again.
    A checklist taller than its room shows a part of its entries and a row with the counts above and below.
    The wheel over the open checklist scrolls it. One notch moves three lines at most, and no more than the checklist shows.
    So each entry can show, also on a short terminal. The wheel elsewhere scrolls the transcript.
55. A turn shows a row for each warning note and each compaction in its `notices` field (040, rule 16), open or folded.
    The console reads them from the turn and reads no steps for them.
    Each of these rows is an item of the cursor. Enter opens it in place to its whole text.
    Where `notices_more` is above zero, one row gives the count.
    An open turn shows them also where its steps cannot be read.
    A step row of an open turn that is one of these notices does not show again.
    An info note shows only in an open turn, as a step row.
56. An answered permission is a mark on the row of its call: the mark of the option kind, its name, and who chose it if not the console.
    The permission has no row of its own where the row of its call shows.
57. One row under the input box replaces the status row and the footer.
    Its left side shows the mark of the session state, the model, the mode, the context percentage and the cost.
    It prints no status word and no tokens.
    The model is the model config option of the agent. Where the agent sends none, it is the stored model of the session row.
    A narrow terminal drops whole items from the left, the least important first: the cost, the context, the mode, the model, then the mark.
    The row shows no key-hint string for a zone.
58. An image, an audio block, a resource without text and a resource link show as one line: `[<type>] <name>`.
    The name is the last part of the URI, else the MIME type, else the title of a link, else its name.
    A change of a shown field draws the line again, also when the text of the turn stays the same.
59. A tool step row has its state mark, then the icon of its kind from `theme::KINDS`, then its subject.
    A kind with no entry there has one fallback icon.
    Consecutive tool steps of kind `read` or `search` in one turn fold into one group row, which shows the subject of each step, cut to the width.
    A group of one kind has the icon of that kind, and a group of both kinds has the group glyph `☰`.
    A folded running turn takes its last three steps (rule 41) before it folds the groups, so it shows at most three steps, in rows or in one group.
    A step of another kind ends a group, and a group of one step is a normal row.
    The state mark of a group is running while a step in it runs, else failed if a step failed, else completed.
    Enter on a group row opens it and shows its steps as rows, and each opens its detail (rule 51).
    An edit row shows `+N -N` from its `diff_stat`, when it has one.
60. The right side of the bottom row shows `? keys`, which drops after every item of the left side.
    `reconnecting` and `ctrl-c again to leave` replace the left side while they apply.
61. `?` opens an overlay that lists every key of the console, in a group for each zone: the input, its box editing, a permission, a question, the transcript, the tabs, and the keys of every zone.
    It opens in the transcript zone and in an empty input box. `?` in a box that holds text types the character.
    The first row names the close and the scroll keys. Up, Down, PgUp, PgDn, Home and End scroll the list where it is taller than the screen, so every key shows on a 60 by 24 terminal.
    The groups stand in columns where the width holds two or more: a column takes 3 columns of indent, 14 of key, the longest description and a gap of 2, and the overlay has as many columns as fit across the width, and one where fewer than two fit.
    Each group stays whole in one column with its keys under its heading, and the groups keep their order down each column, then across. The columns are as short as that order allows, so every key shows with no scroll on a 160 by 50 terminal.
    While the overlay is open the screen shows no text cursor. The cursor returns to the input box when it closes.
    `?` or Esc closes it. The transcript position and the input text stay as they were.

## Acceptance criteria

1. Launch ids are unique (`crates/ariadne-daemon/tests/it/resume.rs::every_launch_of_a_session_reports_under_a_new_id`), and stale reports cannot change later work (`events.rs::an_event_from_a_launch_the_session_has_moved_past_changes_nothing`).
2. Kill and resume keep the session row and conversation (`crates/ariadne-daemon/tests/it/acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`, `::resuming_an_acp_author_replaces_the_agent_and_keeps_the_session`).
3. Resume reuses its row and falls back from a missing worktree (`crates/ariadne-daemon/tests/it/resume.rs::reviving_a_session_revives_it_in_place`, `::a_session_with_a_deleted_worktree_revives_in_the_repository_checkout`).
4. Console input and cancellation work, and the two old event routes are gone: each answers `404` and neither is in the OpenAPI document (`crates/ariadne-daemon/tests/it/acp_console.rs::posted_input_reaches_the_agent_and_queues_behind_a_running_turn`, `::cancelling_a_running_turn_ends_it_as_cancelled`, `::the_old_console_event_routes_are_gone`, `crates/ariadne-daemon/tests/it/far_tasks/source_references.rs::the_retired_console_routes_of_a_worker_session_are_not_forwarded`).
5. Rules 15 to 17: the terminal draws the full-screen console, takes keys, answers a permission, redraws on a resize, obeys the wheel and the click, and keeps the session alive when its socket closes (`crates/ariadne-daemon/tests/it/acp_terminal.rs::the_terminal_draws_the_full_screen_console_at_the_client_size`, `::typed_keys_and_enter_reach_the_stub_agent_as_a_prompt`, `::a_key_answers_a_pending_permission_question`, `::closing_the_socket_leaves_the_session_alive_and_the_session_ending_closes_it`, `::a_relaunch_keeps_the_socket_open_and_a_later_socket_too`, `::a_socket_opened_after_the_session_ended_gets_the_transcript_and_closes`, `::ctrl_c_twice_or_ctrl_d_closes_the_socket_and_the_session_stays_alive`, `::a_resize_redraws_at_the_new_size`, `::a_click_message_opens_and_closes_the_turn_under_it`, `::a_wheel_message_scrolls_the_transcript`). A remote session serves the route through the forwarder (`crates/ariadne-daemon/tests/it/far_tasks/source_references.rs::source_trajectory_stream_and_terminal_connections_reach_the_worker_session`). The OpenAPI document holds the route and its messages (`crates/ariadne-daemon/tests/it/acp_terminal.rs::the_terminal_endpoint_and_its_messages_are_in_the_openapi_document`). A host that reads no stored model, no detail and no subagent steps shows the step text, the reason and no model (`ariadne-console::app::tests::a_host_that_reads_no_detail_shows_the_step_text_and_the_reason`).
8. Session switches preserve the required conversation behavior (`crates/ariadne-daemon/tests/it/switch.rs::a_same_agent_switch_keeps_the_row_and_conversation`, `::a_switched_agent_starts_a_new_session_briefed_on_its_column`, `::a_session_already_switched_is_not_switched_again`).
17. Naming a `subagent_id` cancels that child with its own session id and leaves the session's own turn as it is, and is refused for a child that is unknown, has ended, or never advertised the capability (`crates/ariadne-daemon/tests/it/acp_console.rs::cancelling_an_announced_subagent_sends_session_cancel_with_its_own_id`, `::cancelling_an_unknown_subagent_is_refused`, `::cancelling_an_ended_subagent_is_refused`, `::cancelling_a_subagent_that_does_not_advertise_cancel_is_refused`, `ariadne-daemon::acp_subagents::tests::a_child_may_be_cancelled_only_while_running_and_advertising_it`).

20. A session place survives a store restart (`crates/ariadne-store/tests/session_place.rs::the_session_place_survives_a_store_restart`).
21. A question draws its fields above the input with the option text as written, the keys answer it field by field, Esc declines it, a form with no fields is accepted, text typed on a choice field without a companion is a prompt, and a required field is not left empty (`ariadne-console::app::tests::an_open_question_shows_its_form_above_the_input_and_the_keys_answer_it`, `::the_keys_answer_each_field_and_the_last_enter_sends_the_content`, `::escape_declines_the_open_question`, `::enter_accepts_a_form_without_fields_with_empty_content`, `::text_typed_on_a_choice_field_is_a_prompt_and_answers_nothing`, `::a_required_field_with_no_choice_is_not_taken_empty`, `::left_and_tab_with_text_in_the_box_do_not_change_the_question`, `::a_required_question_left_unanswered_blocks_the_send_until_it_is_answered`).
22.  `ariadne session logs` draws an open question with its options as written and a closed one with its compact answer, and a followed log draws the close under it (`ariadne-console::trajectory::print::tests::a_question_prints_its_options_while_open_and_its_answer_once_closed`, `ariadne-cli::commands::transcript::tests::a_follow_prints_each_item_once_across_snapshots`).

23. Stepper acceptance: paired Claude fields and typed single/multi answers use
`ariadne-console::app::tests::a_choice_and_its_other_companion_are_one_step_and_the_typed_answer_stands_for_the_choice`.
Number fields use `ariadne-console::app::tests::a_number_field_is_one_step_and_sends_its_value`.
Number selection and question navigation use
`ariadne-console::app::tests::a_number_picks_a_choice_and_moving_between_questions_keeps_the_answers`.

24. Required multi-select custom answers contain no unchecked choices
(`ariadne-console::app::tests::a_required_multi_select_answered_with_text_alone_sends_no_unchecked_choice`).

25. Rule 39: `ariadne-cli::commands::console::app::tests::the_console_takes_the_alternate_screen_and_the_mouse_and_gives_both_back`, `::the_terminal_is_given_back_when_ctrl_c_leaves_the_console_and_on_an_error`, `::the_terminal_is_given_back_when_setup_or_drive_panics`, `::terminal_restore_continues_after_a_failed_write`, `ariadne-cli::cli::tests::attach_opens_the_full_screen_console_with_no_flag`, `ariadne-cli::commands::console::terminal::tests::only_a_terminal_on_both_ends_gets_the_full_screen_console`.
26. Rule 40: `ariadne-cli::commands::console::app::tests::an_update_that_skips_a_revision_dials_the_stream_again`, `ariadne-console::app::tests::a_reconnect_shows_no_item_two_times`.
27. Rule 41: `ariadne-console::app::tests::a_turn_shows_its_prompt_one_activity_line_and_its_answer`, `::a_running_turn_shows_its_last_steps_live`, `::a_running_turn_draws_the_whole_answer_and_live_steps`, `::a_streamed_append_updates_the_live_answer`.
28. Rule 42: `ariadne-console::app::tests::an_open_turn_shows_a_row_per_step_with_its_status_kind_subject_and_duration`, `::an_open_running_turn_reads_all_steps_after_it_finishes`.
29. Rule 43: `ariadne-console::app::tests::tab_gives_the_keys_to_the_transcript_and_tab_escape_or_a_typed_character_gives_them_back`.
30. Rule 44: `ariadne-console::app::tests::enter_sends_the_prompt_and_it_shows_queued_until_a_turn_holds_it`, `::up_and_down_move_through_the_rows_then_through_the_prompt_history`, `::page_up_and_page_down_scroll_the_transcript_and_the_end_follows`, `::ctrl_o_opens_and_closes_the_newest_turn`, `::escape_cancels_the_running_turn`, `::a_slash_opens_the_session_commands_and_tab_completes_the_pick`.
31. Rule 45: `ariadne-console::app::tests::the_cursor_walks_turns_and_steps_with_the_arrows_and_j_and_k`, `::enter_and_right_open_the_item_and_left_closes_it_or_goes_to_its_turn`, `::home_and_end_go_to_the_first_and_the_newest_turn`, `::y_copies_the_item_at_the_cursor_to_the_clipboard`, `ariadne-cli::commands::console::app::tests::y_puts_the_text_on_the_clipboard_through_osc_52`.
32. Rule 46: `ariadne-console::app::tests::the_wheel_scrolls_in_both_zones_and_a_click_opens_or_closes_an_item`, `::page_up_and_page_down_scroll_the_transcript_and_the_end_follows`.
33. Rule 47: `ariadne-console::app::tests::a_resize_draws_the_whole_screen_again_and_loses_no_line`, `::the_screen_on_an_ansi_terminal_reads_back_as_the_test_backend_draws_it`.
34. Rule 48: `ariadne-console::app::tests::a_pending_permission_shows_above_the_input_and_enter_sends_the_picked_option`, `::an_open_question_shows_its_form_above_the_input_and_the_keys_answer_it`, `::escape_declines_the_open_question`, `::question_reads_retry_after_failure_and_reconnect`, `::the_selected_permission_option_stays_visible_after_resize_and_answers`, `::the_active_question_and_choice_stay_visible_after_resize_and_answer`.
35. Rule 49: `ariadne-cli::commands::console::app::tests::prompt_history_reaches_past_the_trajectory_page`, `ariadne-console::app::history::tests::the_history_reaches_back_through_the_older_pages_oldest_first`, `::a_page_that_cannot_be_read_ends_the_reach_with_what_it_has`, `ariadne-console::app::tests::a_slash_opens_the_session_commands_and_tab_completes_the_pick`, `::enter_sends_the_prompt_and_it_shows_queued_until_a_turn_holds_it`, `::a_refused_prompt_is_said_under_the_transcript_and_is_no_longer_queued`, `::one_ctrl_c_keeps_the_console_and_the_second_leaves_it`, `::up_and_down_move_through_the_rows_then_through_the_prompt_history`.
36. Rule 50: `ariadne-console::app::tests::the_farewell_prints_at_most_twenty_folded_turns_and_a_hint_to_the_log`, `::a_session_that_has_ended_leaves_after_its_snapshot`, `ariadne-cli::commands::console::app::tests::the_console_closing_says_whether_the_session_still_runs`.
37. Rule 51: `ariadne-console::app::tests::an_open_command_shows_the_command_and_its_output_folded_then_whole`, `::an_open_edit_shows_its_change_as_a_coloured_diff`, `::an_open_read_shows_the_file_in_its_language`, `::enter_and_right_open_the_item_and_left_closes_it_or_goes_to_its_turn`, `ariadne-cli::commands::console::app::tests::the_host_reads_a_detail_and_the_steps_of_a_subagent_from_their_routes`, `ariadne-client::tests::a_step_detail_sends_its_route_and_reads_the_typed_detail`.
38. Rule 52: `ariadne-console::app::tests::r_shows_the_raw_input_and_output_of_the_call_at_the_cursor`.
39. Rule 53, the tab bar: `ariadne-console::app::tests::the_top_row_is_a_tab_bar_of_main_and_each_subagent_in_start_order`, `::a_session_with_no_subagent_shows_no_tab_bar_and_the_transcript_uses_the_row`, `::a_narrow_bar_cuts_the_names_keeps_the_marks_and_marks_tabs_off_the_screen`, `::from_five_columns_the_active_tab_its_mark_and_the_edge_marks_fit_whole`.
    The summary row and the tab: `::a_subagent_in_main_is_one_summary_row_that_does_not_open_in_place`, `::enter_on_a_summary_row_opens_the_tab_of_its_subagent_with_the_rows_of_main`, `::a_click_on_a_summary_row_opens_the_tab_of_its_subagent`, `::a_shown_subagent_tab_is_read_again_when_its_turn_completes`, `ariadne-cli::commands::console::app::tests::the_host_reads_a_detail_and_the_steps_of_a_subagent_from_their_routes`.
    The keys, the places and the input: `ariadne-console::app::tests::ctrl_n_and_ctrl_p_move_between_tabs_and_escape_returns_to_main`, `::each_tab_keeps_its_own_scroll_position_and_cursor_row`, `::the_input_sends_to_the_session_from_a_subagent_tab`.
40. Rule 54: `ariadne-console::app::tests::a_plan_with_an_open_entry_shows_one_line_above_the_input_that_opens_to_the_checklist`, `::every_entry_of_a_checklist_taller_than_its_room_is_reached_with_the_wheel`, `::the_wheel_reaches_every_entry_of_a_plan_on_a_short_terminal`.
41. Rule 55: `ariadne-console::app::tests::warnings_and_compactions_show_in_their_turn_from_its_notices_without_a_step_read`, `::an_open_turn_shows_its_notices_and_their_count_once_whether_its_steps_read_or_not`, `::a_notice_is_an_item_the_cursor_reaches_and_it_opens_in_place_to_its_whole_text`, `::an_info_note_shows_in_the_open_turn_and_not_in_the_folded_one`.
42. Rule 56: `ariadne-console::app::tests::an_answered_permission_is_a_mark_on_the_row_of_its_call`.
43. Rule 57: `ariadne-console::app::tests::the_bottom_row_shows_the_state_mark_model_mode_context_and_cost_and_the_keys_hint`, `::a_narrow_bottom_row_drops_whole_items_in_rank_order_and_the_keys_hint_last`, `::the_bottom_row_names_the_stored_model_where_the_agent_sends_no_model_option`, `::tab_gives_the_keys_to_the_transcript_and_tab_escape_or_a_typed_character_gives_them_back`, `ariadne-cli::commands::console::app::tests::the_stored_model_is_read_from_the_session_row`, `crates/ariadne-daemon/tests/it/acp_terminal.rs::a_wheel_message_scrolls_the_transcript`.
44. Rule 58: `ariadne-console::app::tests::an_image_audio_resource_and_link_show_as_one_line_with_type_and_name`, `::a_changed_image_name_or_link_title_draws_again_while_the_text_stays`.
45. Rule 59: `ariadne-console::app::tests::a_tool_row_starts_with_its_state_mark_and_the_icon_of_its_kind_and_prints_no_kind_word`, `::a_kind_with_no_icon_shows_the_fallback_icon`, `::consecutive_read_and_search_steps_show_as_one_group_row_of_their_subjects`, `::a_group_row_cuts_its_subjects_to_the_width`, `::a_group_is_running_while_a_step_runs_failed_if_one_failed_and_completed_otherwise`, `::enter_on_a_group_row_opens_its_steps_as_rows_that_open_their_detail`, `::an_edit_row_shows_its_subject_and_the_lines_it_added_and_removed`, `::a_group_of_one_kind_has_the_icon_of_that_kind`, `::a_folded_running_turn_shows_its_last_three_steps_even_when_they_group`. Rule 41, no state word on a completed turn: `::a_turn_shows_its_prompt_one_activity_line_and_its_answer`.
46. Rule 60: `ariadne-console::app::tests::a_narrow_bottom_row_drops_whole_items_in_rank_order_and_the_keys_hint_last`, `::reconnecting_and_the_leave_hint_replace_the_left_side_of_the_row_while_they_apply`, `::one_ctrl_c_keeps_the_console_and_the_second_leaves_it`.
47. Rule 61: `ariadne-console::app::tests::a_question_mark_opens_an_overlay_of_every_key_in_groups_by_zone`, `::a_question_mark_in_a_non_empty_input_types_the_character`, `::a_question_mark_or_escape_closes_the_overlay_and_the_screen_stays_as_it_was`, `::the_question_mark_opens_the_overlay_from_the_transcript_zone_too`, `::every_key_of_the_overlay_is_reached_by_scrolling_in_a_60_by_24_terminal`, `::a_wide_overlay_shows_its_groups_whole_in_columns_with_no_scroll`, `::a_narrow_overlay_shows_one_column`, `::the_overlay_hides_the_text_cursor_and_closing_it_brings_the_cursor_back`.

## Sources

- crates/ariadne-console/src/app/mod.rs
- crates/ariadne-console/src/app/detail.rs
- crates/ariadne-console/src/app/status.rs
- crates/ariadne-console/src/diff.rs
- crates/ariadne-cli/src/commands/console/app.rs
- crates/ariadne-daemon/src/http/console.rs
- crates/ariadne-daemon/src/http/terminal.rs
- crates/ariadne-daemon/src/http/source_sessions.rs
- crates/ariadne-console/src/ansi.rs
- crates/ariadne-daemon/src/http/sessions.rs
- crates/ariadne-console/src/app/history.rs
- ui/src/features/sessions/console/console-stream.ts
- ui/src/features/sessions/console/transcript.ts
- ui/src/features/sessions/console/session-console.tsx
