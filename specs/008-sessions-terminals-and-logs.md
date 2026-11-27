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
  - crates/ariadne-console/src/transcript.rs
  - crates/ariadne-console/src/tui/mod.rs
  - crates/ariadne-console/src/tui/blocks.rs
  - crates/ariadne-console/src/tui/picker.rs
  - crates/ariadne-console/src/tui/subagents.rs
  - crates/ariadne-console/src/tui/form.rs
  - crates/ariadne-cli/src/commands/transcript.rs
  - crates/ariadne-console/src/trajectory/print.rs
  - crates/ariadne-console/src/app/tests.rs
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

In: session rows, launches, kill and resume, the console HTTP routes, terminal
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
10. `GET /v1/sessions/{id}/console` returns the newest ordered console snapshot.
11. `GET /v1/sessions/{id}/console/stream` sends the snapshot and later events.
12. `POST /v1/sessions/{id}/console/input` sends or queues one prompt and answers a pending permission request.
13. A finished or unavailable session refuses console input with `409`.
14. `POST /v1/sessions/{id}/console/cancel` cancels a running turn and refuses a session without a running turn with `409`.
15. `GET /v1/sessions/{id}/console/terminal` upgrades a known session to the terminal WebSocket.
    It sends the full-screen console of rules 39 to 59, as terminal bytes: the alternate screen, the mouse and bracketed paste, then the screen.
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
18. See [015](015-desktop-app.md) for the desktop console, which reads the console stream and posts to the input and cancel routes rather than the terminal WebSocket.
19. The CLI attach pane renders the transcript, follows the console stream, and posts input.
20. The attach pane keeps the status, input, and footer rows pinned below its transcript.
21. The console folds updates of an open tool call into its existing transcript block.
22. The console reconnects after a dropped stream and redraws its snapshot without duplicate scrollback.
23. A session switch records the old session and starts the replacement with its handoff.
24. A same-agent switch keeps the conversation and applies the new model and effort.
25. A cross-agent switch starts a new conversation and leaves the old session exited.
26. A cancelled-goal session, a model turned off, or an already switched session refuses switching.
27. See [033](033-telemetry.md) for the daemon's own log: the telemetry store it is read from, and its snapshot and stream routes.
28. The console draws an agent's thought whole, as dimmed markdown; Ctrl-O does not fold it.
29. The console draws the rest of a permission's command in the terminal's own foreground, not dimmed.
30. The console shows a bar above the status row only while the session has a subagent (021 rule 15). The bar lists the parent view, `main`, then each child in announcement order, each with the mark of its state — `●` running, `✓` completed, `✗` failed, `⊘` cancelled, `◇` none — and `?` on the child whose permission question waits. The view on the screen is reversed.
31. Tab moves to the next view and Shift-Tab to the previous one, in bar order, wrapping around, while the slash-command suggestion list is closed. While the list is open, Tab completes the pick. A session with no child takes Tab as it did before.
32. A switch clears the terminal, its scrollback too where the terminal allows, and draws the chosen view whole, in the CLI attach pane.
33. A child's view shows only that child's events. It has no input box, takes no typing, and its footer starts with `read-only`. Esc returns to the parent view, unless the child advertised `cancel` (021 rule 16), where it cancels that child's turn instead and the footer says `esc cancel` rather than `esc back`. Ctrl-O folds and unfolds as in the parent view.
34. The parent view shows a line per child announcement, `◇ subagent <title> · <task>`, and a line per end state, `<mark> subagent <title> <state> · <elapsed>`, timed from the announcement; a child whose first update already ends it gets both lines. A child's own events stay out of it, and a child's command list never replaces the session's suggestion list. A child's permission question is asked in the parent view, since the session answers it, and names the child: `<tool> · asked by subagent <title>`.
35. The status row says `running N subagents` while any child is in state `running`.
36. `POST /v1/sessions/{id}/console/cancel` takes an optional body, `{"subagent_id"}`. Named, it cancels that subagent's turn (021 rule 16) instead of the session's own, which is left as it is; the body omitted, or `subagent_id` omitted, cancels the session's own turn as before. It refuses with `409` a subagent that is unknown, has already ended, or was never said to be cancellable.
37. An open question that the agent asks with its own tool (021 rule 21) of a live session draws in the live area right above the input, and stays there while the blocks written after it go above it and into the scrollback. A question that closes is written to the scrollback once, after the blocks that came before its close. The text the agent writes under an open question streams into one block, as it does with no question. A reconnect's replay keeps the place of a close that came while the stream was down. A question still open when the console exits is written to the scrollback once, after every block. A finished session draws its questions in event order, and a child's view has none of its own.
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
      Esc declines. The footer names these keys.
    - Other field types retain one step per field and their existing validation.
      Empty forms accept Enter. Permission questions take keys first.
    - Closed forms show compact summaries in the console and transcript output.
      Each question shows "<field title, else the message> → <answer>", with custom text merged.
      Declined, withdrawn and cancelled forms show one line.
      Followed logs draw the close when it arrives.

### The full-screen console

`ariadne attach` opens it on a terminal, with no flag.
Its code is `crates/ariadne-console/src/app/`.
`attach` no longer opens the inline pane of rules 19 to 37.
Rule 59 gives the outcome of each of those rules in the full-screen console.

39. On a terminal, `attach` takes the alternate screen, raw mode, the mouse and bracketed paste.
    The console gives the terminal back on each way out: a leave, an error, a panic and Ctrl-C.
    Without a terminal on both ends, `attach` prints the line protocol.
    `attach` has no `--full-screen` flag.
40. The console reads the trajectory stream (040) and draws the client view of the trajectory.
    A drop, a `resync`, or an update that skips a revision shows `reconnecting` and dials the stream again.
    The next snapshot replaces the view. Thus a reconnect shows no item two times.
41. The transcript shows each turn as its prompt, one activity line and its answer.
    The activity line has the fold mark, the state mark, the counts of the turn and its duration.
    The answer is markdown and appears while the turn runs. A daemon prompt shows its first line only.
    A folded running turn shows its last three steps, live.
    The latest message supplies the running answer as text arrives. Its step row keeps the status without repeating the answer.
42. An open turn shows a row for each step under its activity line.
    A row has the status mark, the kind, the subject and the duration.
    The kind of a tool call is the kind of its tool.
    The console reads the complete steps from the steps route (040), including when an open turn finishes.
43. The screen has two zones: the transcript and the input. The input zone has the keys at the start.
    Tab gives the keys to the transcript, with the cursor on the newest turn.
    In the transcript, Tab, Escape or a typed character gives the keys back to the input.
    A typed character goes into the input box. The footer names the keys of the zone.
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
49. The input box, the prompt history, the slash commands, the queued prompts and Ctrl-C work as in the inline pane.
    On a truncated trajectory, attach reads prompt history from the inline console snapshot when available.
    The slash commands are the commands of the session in the trajectory.
    A sent prompt shows `queued` until a new console turn holds its text.
    A refused prompt leaves the queue, and the console shows the refusal under the transcript.
    One Ctrl-C arms the leave and the footer says so. A second Ctrl-C leaves. Ctrl-D leaves at once.
50. On leaving, the console prints the turns that this attach showed, folded, as `ariadne session logs` prints them.
    It prints the newest 20 at most, then a hint to `ariadne session logs <id> --steps`.
    A session that ended before the attach leaves after its snapshot.
51. An open tool call step reads its detail from the detail route (040) and shows it by its kind.
    A command shows its command, its working directory, its output and its exit code.
    An edit shows each file as a coloured diff, with the rules of the inline pane.
    A read shows the path and the line, then the file text coloured by the grammar of its language.
    A search and a fetch show what they looked for and what they found. Any other call shows its content.
    An output keeps its last 12 lines. A diff, a file and raw JSON keep their first 24 lines.
    Each fold shows a count of the lines it left out. A second Enter on the open step shows it whole.
    A change of `detail_rev` reads the detail again.
    A host that cannot read the detail shows the text of the step and the reason.
    Any other open step shows its whole text under its row.
52. In the transcript zone, `r` shows the raw input and the raw output of the tool call at the cursor, as JSON.
    The console reads them with `raw=true`. A second `r` shows the detail again.
53. A subagent step shows its title, its task, its state, its time and its step count.
    Enter opens it in place to its steps, one level under its row. The console reads them with `subagent=<id>` (040, rule 44).
    While the turn runs, the live steps of its lane join them. A step under its subagent does not name its lane.
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
57. The status row under the input shows the status, the mode, the model, the context use, the cost and the tokens.
    The model is the model config option of the agent. Where the agent sends none, it is the stored model of the session row.
    A narrow terminal drops whole items, the least important first: the cost, the tokens, the context, the mode, then the model.
    The status stays to the last.
58. An image, an audio block, a resource without text and a resource link show as one line: `[<type>] <name>`.
    The name is the last part of the URI, else the MIME type, else the title of a link, else its name.
    A change of a shown field draws the line again, also when the text of the turn stays the same.
59. Each rule of the inline pane has this outcome in the full-screen console:
    - Rule 19: replaced by rules 40 and 44. The console reads the trajectory stream and posts the input.
    - Rule 20: replaced by rules 43 and 57. The input box, the status row and the footer are under the transcript.
    - Rule 21: replaced by rule 40. An update of a tool call replaces its step, and its row changes in place.
    - Rule 22: replaced by rule 40. A reconnect shows no item two times.
    - Rules 23 to 27 and 36: they are about sessions and HTTP routes, and stay as they are.
    - Rule 28: replaced by rule 45. An open thought shows its whole markdown, dimmed. Ctrl-O opens the newest turn and folds no thought.
    - Rule 29: replaced by rule 48. A permission draws its call in the tool colour, not dimmed.
    - Rules 30 and 35: ended. The console has no subagent bar and no count of the running subagents. A subagent row shows its state (rule 53).
    - Rule 31: ended. Tab moves the keys between the zones (rule 43).
    - Rule 32: ended. The console draws on the alternate screen (rule 39), and it clears no scrollback.
    - Rule 33: ended. The console has no view of one subagent: a subagent opens to its steps in place (rule 53). Escape cancels the session's own turn (rule 44).
    - Rule 34: replaced by rules 53 and 48. A subagent is a step row, and its permission names it: `<question> · asked by subagent <title>`.
    - Rule 37: replaced by rule 48. An open question shows above the input. A question that closes leaves that place, and its step row stays in its turn. No question goes into a scrollback.
    - Rule 38: replaced by rule 48, which uses the form of rule 38.

## Acceptance criteria

1. Launch ids are unique (`crates/ariadne-daemon/tests/it/resume.rs::every_launch_of_a_session_reports_under_a_new_id`), and stale reports cannot change later work (`events.rs::an_event_from_a_launch_the_session_has_moved_past_changes_nothing`).
2. Kill and resume keep the session row and conversation (`crates/ariadne-daemon/tests/it/acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`, `::resuming_an_acp_author_replaces_the_agent_and_keeps_the_session`).
3. Resume reuses its row and falls back from a missing worktree (`crates/ariadne-daemon/tests/it/resume.rs::reviving_a_session_revives_it_in_place`, `::a_session_with_a_deleted_worktree_revives_in_the_repository_checkout`).
4. Console snapshots, streams, input, and cancellation work (`crates/ariadne-daemon/tests/it/acp_console.rs::the_console_stream_gives_the_snapshot_then_deltas`, `::posted_input_reaches_the_agent_and_queues_behind_a_running_turn`, `::cancelling_a_running_turn_ends_it_as_cancelled`).
5. Rules 15 to 17: the terminal draws the full-screen console, takes keys, answers a permission, redraws on a resize, obeys the wheel and the click, and keeps the session alive when its socket closes (`crates/ariadne-daemon/tests/it/acp_terminal.rs::the_terminal_draws_the_full_screen_console_at_the_client_size`, `::typed_keys_and_enter_reach_the_stub_agent_as_a_prompt`, `::a_key_answers_a_pending_permission_question`, `::closing_the_socket_leaves_the_session_alive_and_the_session_ending_closes_it`, `::a_relaunch_keeps_the_socket_open_and_a_later_socket_too`, `::a_socket_opened_after_the_session_ended_gets_the_transcript_and_closes`, `::ctrl_c_twice_or_ctrl_d_closes_the_socket_and_the_session_stays_alive`, `::a_resize_redraws_at_the_new_size`, `::a_click_message_opens_and_closes_the_turn_under_it`, `::a_wheel_message_scrolls_the_transcript`). A remote session serves the route through the forwarder (`crates/ariadne-daemon/tests/it/far_tasks/source_references.rs::source_console_stream_and_terminal_connections_reach_the_worker_session`). The OpenAPI document holds the route and its messages (`crates/ariadne-daemon/tests/it/acp_terminal.rs::the_terminal_endpoint_and_its_messages_are_in_the_openapi_document`). A host that reads no stored model, no detail and no subagent steps shows the step text, the reason and no model (`ariadne-console::app::tests::a_host_that_reads_no_detail_shows_the_step_text_and_the_reason`).
6. Tool updates share a transcript block (`ariadne-console::transcript::tests::updates_of_one_call_fold_into_it_and_the_last_dates_its_end`).
7. A reconnect redraws without duplicate scrollback (`ariadne-console::tui::tests::a_reconnect_redraws_the_fresh_snapshot_without_repeating_the_scrollback`).
8. Session switches preserve the required conversation behavior (`crates/ariadne-daemon/tests/it/switch.rs::a_same_agent_switch_keeps_the_row_and_conversation`, `::a_switched_agent_starts_a_new_session_briefed_on_its_column`, `::a_session_already_switched_is_not_switched_again`).
9. A thought is drawn whole, as markdown (`ariadne-console::tui::blocks::tests::a_thought_draws_its_markdown_whole`, `::ctrl_o_draws_a_daemon_prompt_whole_and_a_thought_is_whole_either_way`).
10. A permission's command is drawn above its options, not dimmed (`ariadne-console::tui::picker::tests::a_permission_question_draws_the_call_above_its_options`).
11. The bar lists the views in announcement order with their marks, and a session without a child has no bar and keeps its Tab (`ariadne-console::tui::subagents::tests::the_bar_lists_the_parent_and_each_child_in_announcement_order`, `::a_session_without_a_child_has_no_bar_and_tab_switches_nothing`).
12. Tab and Shift-Tab walk the views, and Tab completes while the suggestion list is open (`ariadne-console::tui::subagents::tests::tab_and_shift_tab_walk_the_views_in_bar_order`, `::tab_completes_while_the_suggestion_list_is_open`).
13. A switch clears the terminal and draws the view whole (`ariadne-console::tui::subagents::tests::a_switch_clears_the_terminal_and_draws_the_view_whole`).
14. A child's view is read-only, Esc returns, Ctrl-O folds, and each event lands in its own view (`ariadne-console::tui::subagents::tests::a_childs_view_is_read_only_and_esc_returns_to_the_parent`, `::live_events_land_in_their_own_views_whichever_is_shown`).
15. The parent view has a line per announcement and per end, and the child's question names it (`ariadne-console::tui::subagents::tests::the_parent_view_has_a_line_per_announcement_and_per_end`, `ariadne-console::transcript::tests::a_subagents_question_names_the_child`, `ariadne-console::tui::subagents::tests::a_child_first_heard_of_at_its_end_has_both_lines`, `::a_childs_commands_leave_the_parents_list_alone`).
16. The status row counts the running subagents (`ariadne-console::tui::subagents::tests::the_status_row_counts_the_running_subagents`).
17. Naming a `subagent_id` cancels that child with its own session id and leaves the session's own turn as it is, and is refused for a child that is unknown, has ended, or never advertised the capability (`crates/ariadne-daemon/tests/it/acp_console.rs::cancelling_an_announced_subagent_sends_session_cancel_with_its_own_id`, `::cancelling_an_unknown_subagent_is_refused`, `::cancelling_an_ended_subagent_is_refused`, `::cancelling_a_subagent_that_does_not_advertise_cancel_is_refused`, `ariadne-daemon::acp_subagents::tests::a_child_may_be_cancelled_only_while_running_and_advertising_it`).
18. Esc in a child's view cancels it when it advertised the capability, and the footer names the key either way (`ariadne-console::tui::subagents::tests::esc_cancels_a_child_that_advertises_it_instead_of_switching_back`, `::esc_posts_a_subagent_cancel_through_the_sink`, `::a_childs_view_is_read_only_and_esc_returns_to_the_parent`).
19. An open question stays above the input, a closed one goes into the scrollback once after the blocks before its close, one open at the exit is written once, and a finished session keeps event order (`ariadne-console::tui::viewport::tests::an_open_question_stays_over_the_input_while_later_blocks_go_into_the_scrollback`, `::a_closed_question_goes_into_the_scrollback_once_after_the_blocks_before_its_close`, `::a_question_open_at_the_close_is_written_once_after_every_block`, `::a_finished_session_draws_its_questions_in_event_order`); chunks under an open question make one block, and a replay keeps a close's place (`ariadne-console::tui::viewport::tests::chunks_under_an_open_question_make_one_block_that_is_written_whole`, `::a_question_closed_while_the_stream_was_down_keeps_its_close_position`).

20. A session place survives a store restart (`crates/ariadne-store/tests/session_place.rs::the_session_place_survives_a_store_restart`).
21. A question draws its fields above the input with the option text as written, the keys answer it field by field, Esc declines it, a form with no fields is accepted, text typed on a choice field without a companion is a prompt, a required field is not left empty, and a closed question shows each answer (`ariadne-console::tui::form::tests::an_open_question_draws_its_fields_and_the_option_text_as_written_above_the_input`, `::the_keys_answer_each_field_and_the_last_enter_sends_the_content`, `::escape_declines_the_question`, `::enter_accepts_a_form_without_fields_with_empty_content`, `::text_typed_on_a_choice_field_is_sent_as_a_prompt`, `::a_required_field_with_no_choice_left_is_not_taken`, `::left_with_text_in_the_box_does_not_change_the_question`, `::skipped_required_questions_block_submission_until_answered`, `::a_closed_question_shows_each_answer_by_the_option_text`).
22.  `ariadne session logs` draws an open question with its options as written and a closed one with its compact answer, and a followed log draws the close under it (`ariadne-console::trajectory::print::tests::a_question_prints_its_options_while_open_and_its_answer_once_closed`, `ariadne-cli::commands::transcript::tests::a_follow_prints_each_item_once_across_snapshots`).

23. Stepper acceptance: paired Claude fields and typed single/multi answers use
`ariadne-console::tui::form::tests::claude_companions_are_one_step_and_custom_answers_merge`.
Number fields use `ariadne-console::tui::form::tests::a_number_field_remains_a_single_step_and_sends_its_value`.
Number selection and question navigation use
`ariadne-console::tui::form::tests::numbers_answer_and_navigation_preserves_answers`.

24. Required multi-select custom answers contain no unchecked choices
(`ariadne-console::tui::form::tests::required_multi_custom_answers_do_not_add_unselected_choices`).

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
35. Rule 49: `ariadne-cli::commands::console::app::tests::prompt_history_reaches_past_the_trajectory_page`, `ariadne-console::app::tests::a_slash_opens_the_session_commands_and_tab_completes_the_pick`, `::enter_sends_the_prompt_and_it_shows_queued_until_a_turn_holds_it`, `::a_refused_prompt_is_said_under_the_transcript_and_is_no_longer_queued`, `::one_ctrl_c_keeps_the_console_and_the_second_leaves_it`, `::up_and_down_move_through_the_rows_then_through_the_prompt_history`.
36. Rule 50: `ariadne-console::app::tests::the_farewell_prints_at_most_twenty_folded_turns_and_a_hint_to_the_log`, `::a_session_that_has_ended_leaves_after_its_snapshot`, `ariadne-cli::commands::console::app::tests::the_console_closing_says_whether_the_session_still_runs`.
37. Rule 51: `ariadne-console::app::tests::an_open_command_shows_the_command_and_its_output_folded_then_whole`, `::an_open_edit_shows_its_change_as_a_coloured_diff`, `::an_open_read_shows_the_file_in_its_language`, `::enter_and_right_open_the_item_and_left_closes_it_or_goes_to_its_turn`, `ariadne-cli::commands::console::app::tests::the_host_reads_a_detail_and_the_steps_of_a_subagent_from_their_routes`, `ariadne-client::tests::a_step_detail_sends_its_route_and_reads_the_typed_detail`.
38. Rule 52: `ariadne-console::app::tests::r_shows_the_raw_input_and_output_of_the_call_at_the_cursor`.
39. Rule 53: `ariadne-console::app::tests::a_subagent_shows_its_title_task_state_time_and_steps_and_opens_to_them`, `ariadne-cli::commands::console::app::tests::the_host_reads_a_detail_and_the_steps_of_a_subagent_from_their_routes`, `ariadne-console::app::tests::an_open_subagent_is_read_again_when_its_turn_completes`.
40. Rule 54: `ariadne-console::app::tests::a_plan_with_an_open_entry_shows_one_line_above_the_input_that_opens_to_the_checklist`, `::every_entry_of_a_checklist_taller_than_its_room_is_reached_with_the_wheel`, `::the_wheel_reaches_every_entry_of_a_plan_on_a_short_terminal`.
41. Rule 55: `ariadne-console::app::tests::warnings_and_compactions_show_in_their_turn_from_its_notices_without_a_step_read`, `::an_open_turn_shows_its_notices_and_their_count_once_whether_its_steps_read_or_not`, `::a_notice_is_an_item_the_cursor_reaches_and_it_opens_in_place_to_its_whole_text`, `::an_info_note_shows_in_the_open_turn_and_not_in_the_folded_one`.
42. Rule 56: `ariadne-console::app::tests::an_answered_permission_is_a_mark_on_the_row_of_its_call`.
43. Rule 57: `ariadne-console::app::tests::the_status_row_shows_the_state_of_the_session_and_drops_whole_items_when_narrow`, `::the_status_row_names_the_stored_model_where_the_agent_sends_no_model_option`, `ariadne-cli::commands::console::app::tests::the_stored_model_is_read_from_the_session_row`, `crates/ariadne-daemon/tests/it/acp_terminal.rs::a_wheel_message_scrolls_the_transcript`.
44. Rule 58: `ariadne-console::app::tests::an_image_audio_resource_and_link_show_as_one_line_with_type_and_name`, `::a_changed_image_name_or_link_title_draws_again_while_the_text_stays`.
45. Rule 59: rules 19 and 22 by the tests of rules 40 and 44 (criteria 26 and 30). Rule 20 by `ariadne-console::app::tests::the_status_row_shows_the_state_of_the_session_and_drops_whole_items_when_narrow`. Rule 21 by `ariadne-console::app::tests::an_update_of_a_call_changes_its_row_in_place`. Rule 28 by `::an_open_thought_shows_its_whole_markdown_dimmed`. Rules 29 and 34 by `::a_permission_draws_its_call_undimmed_and_names_the_subagent_that_asks` and `::a_subagent_shows_its_title_task_state_time_and_steps_and_opens_to_them`. Rules 30 to 33 and 35 by `::tab_gives_the_keys_to_the_transcript_and_tab_escape_or_a_typed_character_gives_them_back`, `::escape_cancels_the_running_turn` and `ariadne-cli::commands::console::app::tests::the_console_takes_the_alternate_screen_and_the_mouse_and_gives_both_back`. Rules 37 and 38 by `ariadne-console::app::tests::an_open_question_shows_its_form_above_the_input_and_the_keys_answer_it`.

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
- crates/ariadne-console/src/tui/mod.rs
- crates/ariadne-console/src/tui/subagents.rs
- ui/src/features/sessions/console/console-stream.ts
- ui/src/features/sessions/console/transcript.ts
- ui/src/features/sessions/console/session-console.tsx
