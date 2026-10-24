---
id: sessions-terminals-and-logs
status: current
updated: 2026-10-24
areas: [daemon, cli, ui]
commits: [e4816cf6, 39937143, a69b953f, 7880c022, 29e6d84e, 1b09ac10, a4d7da95]
tests:
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
---

# Sessions, terminals and logs

How Ariadne keeps agent sessions and lets clients read, control, and switch
them.

## Scope

In: session rows, launches, kill and resume, the console HTTP routes, terminal
WebSockets, CLI attach, and session switches.

Out: ACP process ownership (021), task transitions (001), and prompt wording
(006).

## Behavior

1. A staffed session belongs to a goal and seat, and a column session also belongs to its task.
2. A session stores its worktree, model, optional effort, and agent session id.
3. A session is starting, running, idle, exited, or failed.
4. A task reuses one session for each column agent, and a goal reuses one orchestrator session.
5. Each launch has a new launch id, which is stored before the agent starts.
6. A stale launch report cannot update a later launch of the same session.
7. Killing a live session kills its process and leaves the row resumable.
8. Resume revives the same row and conversation, using the repository checkout when its worktree is absent.
9. A session without an agent conversation cannot resume, and a cancelled goal session cannot revive.
10. `GET /v1/sessions/{id}/console` returns the newest ordered console snapshot.
11. `GET /v1/sessions/{id}/console/stream` sends the snapshot and later events.
12. `POST /v1/sessions/{id}/console/input` sends or queues one prompt and answers a pending permission request.
13. A finished or unavailable session refuses console input with `409`.
14. `POST /v1/sessions/{id}/console/cancel` cancels a running turn and refuses a session without a running turn with `409`.
15. `GET /v1/sessions/{id}/console/terminal` upgrades a known session to the terminal WebSocket.
16. Closing the terminal socket leaves the session alive, and session completion closes its sockets.
17. The terminal accepts resize, key, and paste messages and redraws at the client size.
18. See [015](015-desktop-app.md) for the desktop terminal client.
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
32. A switch clears the terminal, its scrollback too where the terminal allows, and draws the chosen view whole, in the CLI attach pane and in the terminal WebSocket alike.
33. A child's view shows only that child's events. It has no input box, takes no typing, and its footer starts with `read-only`. Esc returns to the parent view, and Ctrl-O folds and unfolds as in the parent view.
34. The parent view shows a line per child announcement, `◇ subagent <title> · <task>`, and a line per end state, `<mark> subagent <title> <state> · <elapsed>`, timed from the announcement; a child whose first update already ends it gets both lines. A child's own events stay out of it, and a child's command list never replaces the session's suggestion list. A child's permission question is asked in the parent view, since the session answers it, and names the child: `<tool> · asked by subagent <title>`.
35. The status row says `running N subagents` while any child is in state `running`.

## Acceptance criteria

1. Launch ids are unique (`crates/ariadne-daemon/tests/it/resume.rs::every_launch_of_a_session_reports_under_a_new_id`), and stale reports cannot change later work (`events.rs::an_event_from_a_launch_the_session_has_moved_past_changes_nothing`).
2. Kill and resume keep the session row and conversation (`crates/ariadne-daemon/tests/it/acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`, `::resuming_an_acp_author_replaces_the_agent_and_keeps_the_session`).
3. Resume reuses its row and falls back from a missing worktree (`crates/ariadne-daemon/tests/it/resume.rs::reviving_a_session_revives_it_in_place`, `::a_session_with_a_deleted_worktree_revives_in_the_repository_checkout`).
4. Console snapshots, streams, input, and cancellation work (`crates/ariadne-daemon/tests/it/acp_console.rs::the_console_stream_gives_the_snapshot_then_deltas`, `::posted_input_reaches_the_agent_and_queues_behind_a_running_turn`, `::cancelling_a_running_turn_ends_it_as_cancelled`).
5. Terminal input, permission selection, lifecycle, and resize work (`crates/ariadne-daemon/tests/it/acp_terminal.rs::typed_keys_and_enter_reach_the_stub_agent_as_a_prompt`, `::a_key_answers_a_pending_permission_question`, `::closing_the_socket_leaves_the_session_alive_and_the_session_ending_closes_it`, `::a_resize_redraws_at_the_new_size`).
6. Tool updates share a transcript block (`ariadne-console::transcript::tests::updates_of_one_call_fold_into_it_and_the_last_dates_its_end`).
7. A reconnect redraws without duplicate scrollback (`ariadne-console::tui::tests::a_reconnect_redraws_the_fresh_snapshot_without_repeating_the_scrollback`).
8. Session switches preserve the required conversation behavior (`crates/ariadne-daemon/tests/it/switch.rs::a_same_agent_switch_keeps_the_row_and_conversation`, `::a_switched_agent_starts_a_new_session_briefed_on_its_column`, `::a_session_already_switched_is_not_switched_again`).
9. A thought is drawn whole, as markdown (`ariadne-console::tui::blocks::tests::a_thought_draws_its_markdown_whole`, `::ctrl_o_draws_a_daemon_prompt_whole_and_a_thought_is_whole_either_way`).
10. A permission's command is drawn above its options, not dimmed (`ariadne-console::tui::picker::tests::a_permission_question_draws_the_call_above_its_options`).
11. The bar lists the views in announcement order with their marks, and a session without a child has no bar and keeps its Tab (`ariadne-console::tui::subagents::tests::the_bar_lists_the_parent_and_each_child_in_announcement_order`, `::a_session_without_a_child_has_no_bar_and_tab_switches_nothing`).
12. Tab and Shift-Tab walk the views, and Tab completes while the suggestion list is open (`ariadne-console::tui::subagents::tests::tab_and_shift_tab_walk_the_views_in_bar_order`, `::tab_completes_while_the_suggestion_list_is_open`).
13. A switch clears the terminal and draws the view whole (`ariadne-console::tui::subagents::tests::a_switch_clears_the_terminal_and_draws_the_view_whole`, `crates/ariadne-daemon/tests/it/acp_terminal.rs::tab_switches_the_terminal_to_a_subagents_view_and_esc_back`).
14. A child's view is read-only, Esc returns, Ctrl-O folds, and each event lands in its own view (`ariadne-console::tui::subagents::tests::a_childs_view_is_read_only_and_esc_returns_to_the_parent`, `::live_events_land_in_their_own_views_whichever_is_shown`).
15. The parent view has a line per announcement and per end, and the child's question names it (`ariadne-console::tui::subagents::tests::the_parent_view_has_a_line_per_announcement_and_per_end`, `ariadne-console::transcript::tests::a_subagents_question_names_the_child`, `ariadne-console::tui::subagents::tests::a_child_first_heard_of_at_its_end_has_both_lines`, `::a_childs_commands_leave_the_parents_list_alone`).
16. The status row counts the running subagents (`ariadne-console::tui::subagents::tests::the_status_row_counts_the_running_subagents`).

## Sources

- crates/ariadne-daemon/src/http/console.rs
- crates/ariadne-daemon/src/http/terminal.rs
- crates/ariadne-daemon/src/http/sessions.rs
- crates/ariadne-console/src/tui/mod.rs
- crates/ariadne-console/src/tui/subagents.rs
- ui/src/features/sessions/session-terminal.tsx
