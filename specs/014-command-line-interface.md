---
id: command-line-interface
status: current
updated: 2026-09-11
areas: [cli]
commits: [3dcba5f1, e94647fd, 3cd70453, 9f7fa36b, 1a862dfe, 87fa62cf, 03f9c8b7, 29e6d84e, 1b09ac10, 7fe184e9]
tests:
  - crates/ariadne-cli/src/cli/tests.rs
  - crates/ariadne-cli/src/output.rs
  - crates/ariadne-cli/src/commands/models.rs
  - crates/ariadne-cli/src/commands/session.rs
  - crates/ariadne-cli/src/commands/task.rs
  - crates/ariadne-cli/src/error.rs
  - crates/ariadne-cli/src/complete.rs
  - crates/ariadne-daemon/tests/doctor.rs
  - crates/ariadne-cli/src/commands/doctor.rs
  - crates/ariadne-cli/src/commands/doctor/checks.rs
  - crates/ariadne-cli/src/commands/doctor/agents.rs
  - crates/ariadne-cli/src/commands/events.rs
  - crates/ariadne-cli/src/output/table.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/src/commands/agent.rs
  - crates/ariadne-cli/src/commands/skill.rs
  - crates/ariadne-cli/src/commands/console.rs
---

# Command-line interface

`ariadne`: the surface a person drives the daemon from. One verb per action, a
help screen that explains itself, and a failure that fits on one line.

## Scope

In: the command tree and its shape, display and listing flags, status
filters, model and effort flags, what a failure prints and exits with, shell
completions, and `ariadne doctor`.

Out: the daemon endpoints behind the commands (012), and the MCP server the
same binary also serves (013).

## Behavior

1. Every user-facing action except project memory exists both here and in the
   desktop app. Project memory is currently CLI-only (019).
2. The tree is one verb per action, grouped by entity — `daemon`, `agent`,
   `models`, `skill`, `repo`, `goal`, `task`, `session`, `events`,
   `attention`, `memory`, `attach`, `doctor`, `completions`, plus the one
   hidden command the agents use (`mcp serve`). Nothing in the tree launches
   an agent or reports on one's behalf: the daemon's ACP runtime does both
   (021).
3. The root and every group share one help-screen shape, and no help screen
   leaks the endpoint of the shell it runs in.
4. Display flags (`--format`, and the listing flags) parse on either side of
   the subcommand, and are advertised exactly where they are honoured — never
   on a command that would ignore them.
5. A listing hides finished work behind the same flag everywhere (`--all`).
6. A status filter takes only the values the daemon knows, spelled in kebab or
   in snake case, several on one flag; a value that is no spelling of one
   lists the real ones.
7. A model is chosen for every agent on the line, with an effort beside it,
   and it is required: `goal create --model` and the `=MODEL` half of every
   `--author`/`--reviewer` slot must be written, a bare agent names no model,
   and `task update --model` refuses `default` — only `--effort` still takes
   that word. A missing model, a model naming no agent, an effort with no
   meaning and a reviewer slot missing a half are usage errors, refused
   before anything is sent. Which agents exist is the daemon's registry: an
   agent id is sent as typed, and the daemon refuses one it does not hold
   (011).
8. The empty list is a flag of its own wherever a repeatable flag names one,
   since a repeatable flag cannot be given zero times on purpose:
   `--no-reviewer` for a task with nothing to review, `--clear-depends-on` for
   one with nothing to wait for.
9. Every judgement the orchestrator makes about a task can be made from here
   too: how it ends (`task create|update --landing`), whether it is reviewed
   (`--reviewer`, `--no-reviewer`), and whether the goal is over
   (`goal complete`).
10. What the agents said to each other is readable from here: `task messages`
    lists the whole channel of a task, oldest first (018); `--full` prints
    each one whole, through `$PAGER`, since the table cuts a body to its
    opening.
11. `task history` is a table too: each row is one transition, `from` and
    `to` painted with the same status glyphs every other status cell carries.
12. `ariadne events` prints one line per event, `time · kind · subject ·
    detail`. An agent event's detail is the `summary` the daemon builds onto
    its DTO (012), and a recorded event reads the same as a live one.
13. A failure prints `error: <sentence>` and nothing else: no `Caused by:`
   block, no transport detail, no repeated envelope. Attach failures keep
   recovery commands in the rendered hint on that same line. `--format json`
   prints the daemon's envelope instead, so a script keeps the status and code
   the human line drops.
14. The exit code says what kind of failure it was, and every kind has one of
   its own; it is documented in `ariadne --help`.
15. Completions are generated for bash and zsh and complete against live data:
    candidates newest first, live sessions before ended ones when attaching,
    the efforts an entry lists and no others.
16. `ariadne doctor` answers why the daemon will not start — including a
    database written by a release whose migrations this one no longer ships,
    which it names along with the file to delete (016).
17. A tool is checked for its version as well as its presence where a version
    is what decides: git below 2.42 has no `worktree add --orphan` and so
    cannot start a task in a repository with no commits (002), which is a
    warning naming that one case, on this PATH and on the daemon's alike.
18. `skill ls` marks an orchestrator-only skill while leaving it available to
    inspect, edit and reset.
19. `doctor` reports every ACP registry entry from the daemon's cached probe.
    It shows measured capabilities, every degraded feature, and the rejection
    reason when the agent cannot satisfy the required contract. One ready
    agent is enough; a registry with none is a failure, since no session can
    be spawned. Beside the agents it checks `git`, `gh` and `glab`, on this
    PATH and on the daemon's.
20. Each `doctor` section is a shared output table: its check, verdict and
    detail columns fit the terminal, and `--no-trunc` prints their cells whole.
21. `task ls`, `goal ls`, `session ls` and `attention` take `--watch`: the
    table is redrawn whole on every event the command cares about, coalesced
    over a short settle window so one change is one redraw, until Ctrl-C.
    Every filter the command takes still narrows what a redraw shows, the
    redraw escapes only reach a real terminal, and `--watch` is advertised
    only on these four commands.
22. A screen of several tables is fitted once, across every group of rows:
    `ariadne attention` prints a section per goal, and a column is the same
    width under every heading — on a `--watch` redraw too. A `--columns`
    naming a column the table does not have is refused once, before any of
    the screen is printed.
23. A heading is one style everywhere: a section heading and the column header
    of a table are both bold and uppercase. `-q` prints the first cell of
    every row of every group, and nothing else — and, like every other `-q`
    listing, it reads no `--columns` and so refuses none.
24. Human mutation output is one styled line. Quiet mutation output is only
    the affected id. Inspect keys use lowercase space-separated words. A row's
    subject column is `title`, except that the agent listing keeps `agent`.
    Boolean columns use the shared `yes_no` wording. Every empty listing states
    what is empty, then gives the next command when one exists.
25. `ariadne agent ls|update` lists and edits the flags each registry agent
    is launched with, keyed by its registry id. `update` takes a flag list, a
    clear, or a reset, and only one of them, and a flag value that reads like
    a flag of the CLI's own is taken as it is.
26. `ariadne session discover` lists filtered pages of the stored sessions of
    every ACP agent that can list them, and names each agent that cannot with
    its reason. It sends `--agent`, `--dir`, `--since`, `--until`, `--search`,
    `--limit`, `--cursor` and `--refresh` to the daemon; `--search` becomes
    `q`, and `--agent` completes registry agent ids. A date activity bound is
    the start of its UTC day for `--since` and the end for `--until`.
    The table ends with `<shown> of <total> sessions` and a reusable next-page
    command when the daemon returns a cursor. `--all` follows every cursor
    into one table and cannot be combined with `--cursor`. JSON preserves the
    page object, while quiet output prints its session ids.
    `ariadne session adopt` assigns one to a ready task through the same REST
    surface (020).
27. `ariadne memory ls|search|delete` reads and removes active repository
    memories. Each command names the repository by id or path (019).
28. `ariadne attach`, `goal attach` and `task attach` open the console of the
    session an id names, revived first when it is gone: it renders the event
    transcript, submits each typed line as a prompt, and lists permission
    choices for numeric answers. `session send` posts one line to that same
    console input. `session logs` prints the transcript, and `session logs -f`
    follows the console's event stream.

## Acceptance criteria

- The command tree is well formed and every command is classified
  (`cli/tests.rs::the_command_tree_is_well_formed`,
  `::every_command_in_the_tree_is_classified`).
- The root and every group are one help-screen shape
  (`::the_root_and_every_group_are_one_help_screen_shape`), and no help screen
  leaks the endpoint (`::no_help_screen_leaks_the_endpoint_of_the_shell_it_runs_in`).
- `--format` and the listing flags are advertised exactly where they are
  honoured (`::format_is_advertised_exactly_where_it_is_honored`,
  `::the_listing_flags_are_advertised_exactly_where_they_are_honored`), and
  parse on either side (`::the_display_flags_parse_on_either_side_of_the_subcommand`).
- A status is spelled in kebab or snake, several ride on one flag, and a
  non-status lists the real ones
  (`::a_status_is_spelled_in_kebab_or_in_snake`, `::several_statuses_ride_on_one_flag`,
  `::a_status_that_is_no_spelling_of_one_lists_the_real_ones`).
- A model and an effort can be chosen for every agent on the line
  (`cli/tests.rs::a_model_can_be_chosen_for_every_agent_on_the_line`,
  `::an_effort_can_be_chosen_beside_every_model`).
- Model and effort misuse is a usage error
  (`::a_model_naming_no_agent_is_a_usage_error`,
  `::an_effort_that_says_nothing_is_a_usage_error`,
  `::a_reviewer_that_names_no_real_agent_is_a_usage_error`), and so is a line
  with no model (`::a_line_with_no_model_is_a_usage_error`), while an agent id
  is the daemon's to check (`::an_agent_id_is_the_daemons_to_check`).
- `models ls` narrows to an agent and `models show` takes and refuses a model
  the way `--model` does
  (`cli/tests.rs::models_ls_takes_an_agent_to_narrow_the_catalogue`,
  `::models_show_takes_a_model_in_the_spelling_dash_dash_model_takes`).
- Completion offers the efforts an entry lists and no others
  (`complete.rs::an_entry_offers_the_efforts_it_lists_and_no_others`).
- `ariadne events` prints the daemon's summary in an agent event's detail
  (`commands/events.rs::an_event_reads_as_time_kind_subject_and_detail`,
  `::an_agent_event_reads_the_same_recorded_as_it_does_live`).
- `task history` paints `from` and `to`, and a row carries a dash for a
  transition with no reason
  (`commands/task.rs::history_paints_the_from_and_to_statuses`,
  `::a_history_row_carries_the_transition_and_a_dash_for_no_reason`).
- `task messages --full` prints a message's header and its whole body, every
  message of the channel, not just the first
  (`commands/task.rs::a_full_message_carries_its_header_and_its_whole_body`).
- A failure is one line, attach hints stay on that line, and JSON keeps the
  envelope (`error.rs::a_bare_message_is_the_whole_line`,
  `::an_attach_failure_is_one_line_with_recovery_commands`,
  `::a_local_failure_reads_as_context_then_cause`,
  `::json_output_keeps_the_machine_readable_half`), with an exit code per kind
  (`::every_kind_of_failure_has_an_exit_code_of_its_own`).
- Completion candidates come out newest first and live sessions first
  (`complete.rs::candidates_come_out_newest_first`,
  `::attaching_offers_live_sessions_first_and_ended_ones_last`).
- `doctor` answers for a database from before the squashed schema
  (`checks.rs::a_database_from_before_the_squash_fails_the_check`).
- `doctor` reports every registry agent, the tools a session and a published
  task need, and a worktree root it cannot write
  (`doctor.rs::every_registry_agent_is_reported_with_what_discovery_made_of_it`,
  `::the_tools_a_session_and_a_published_task_need_are_reported`,
  `::a_worktree_root_the_daemon_cannot_write_is_reported_as_such`,
  `commands/doctor/agents.rs::the_daemon_report_is_read_for_its_tools_the_same_way`), with
  details truncated to a narrow terminal unless `--no-trunc` asks for them
  whole (`commands/doctor.rs::a_narrow_terminal_truncates_doctor_details`,
  `::no_trunc_keeps_doctor_details_and_columns_whole`).
- `doctor` reports ACP probe rejections and degraded capabilities
  (`commands/doctor/agents.rs::acp_probe_results_show_rejections_and_gaps`),
  and fails only where no agent is ready
  (`::no_ready_agent_fails_the_report_and_one_is_enough`).
- A git below the floor is a warning that names what it cannot do, and a
  version line is read down to its major and minor
  (`checks.rs::a_git_below_the_floor_is_a_warning_about_repositories_with_no_commits`,
  `::a_version_line_reads_down_to_its_major_and_minor`).
- The skill listing marks an orchestrator-only skill
  (`skill.rs::a_listing_marks_an_orchestrator_only_skill`).
- `--watch` is advertised on exactly `task ls`, `goal ls`, `session ls` and
  `attention`, and nowhere else
  (`cli/tests.rs::the_watch_flag_is_advertised_exactly_where_it_is_honored`).
- Several row groups are fitted together, and drop and cut the same columns
  (`table.rs::columns_are_fitted_once_across_every_group`,
  `::every_group_drops_and_cuts_the_same_columns`), which is what aligns the
  attention board across its goals
  (`attention.rs::the_columns_align_across_every_goal_of_the_board`).
- A `--columns` naming no column is refused before anything is printed
  (`table.rs::a_bad_column_is_refused_before_a_group_is_rendered`,
  `attention.rs::a_bad_columns_flag_is_refused_before_any_table`).
- A section heading and a column header are one style
  (`table.rs::a_header_is_printed_in_the_one_heading_style`,
  `attention.rs::the_goal_heading_is_printed_in_the_one_heading_style`).
- The board's `-q` is the ids of every group, from the same `quiet_lines`
  every listing pipes through
  (`attention.rs::quiet_output_is_the_ids_of_every_group`), and only a run
  that prints a table refuses a `--columns`
  (`::a_columns_flag_is_refused_only_where_a_table_is_printed`).
- Mutation lines and empty listings share their output forms
  (`output.rs::quiet_mutations_print_only_the_id`,
  `::an_empty_state_puts_the_next_command_on_its_own_line`).
- Quiet output parses on mutations (`cli/tests.rs::quiet_parses_after_a_mutation`).
- Inspect keys contain no underscores
  (`task.rs::the_inspect_block_types_its_id_title_and_status`).
- Session and model subject columns use `title`, and model booleans use
  `yes` or `no` (`session.rs::the_session_subject_column_is_title`,
  `models.rs::the_description_drops_before_the_efforts_do`,
  `::a_row_stars_the_default_effort_and_dashes_what_is_unsaid`).
- `agent update` takes flags, a clear or a reset but only one, and keeps a
  flag that looks like a flag as it is
  (`cli/tests.rs::updating_an_agent_takes_flags_or_clear_or_reset_but_only_one`,
  `::an_agent_flag_that_looks_like_a_flag_is_taken_as_it_is`), and its
  listing keeps the `agent` column name
  (`agent.rs::the_agent_keeps_the_agent_column_name`).
- `session discover` sends every filter and page flag with UTC date bounds,
  follows all pages without repeating a session, prints the count and the
  reusable next-page command only when one exists, and refuses `--all` with
  `--cursor`
  (`session.rs::every_discover_flag_reaches_its_query_parameter`,
  `::a_date_is_the_utc_day_boundary_for_discovery`,
  `::all_fetches_every_page_and_keeps_each_session_once`,
  `::a_next_cursor_prints_the_command_for_the_next_page`,
  `::the_last_page_prints_no_next_command`,
  `::the_discovery_count_is_shown_over_the_total`,
  `cli/tests.rs::discover_takes_filters_pages_refresh_and_all`,
  `::discover_all_and_cursor_are_exclusive`). It names each agent that cannot
  list sessions with its reason
  (`session.rs::an_agent_without_the_capability_is_named_with_its_reason`).
- The memory commands are classified like other lists and mutations
  (`cli/tests.rs::every_command_in_the_tree_is_classified`), and delete takes
  its entry and repository (`::memory_delete_takes_the_entry_and_its_repository`).
- The console renders a stub-agent transcript and submits typed input, and
  its permission question submits the selected option
  (`commands/console.rs::a_console_renders_a_stub_agent_transcript_and_delivers_an_input_line`,
  `::a_permission_question_renders_and_delivers_the_selected_answer`);
  `session send` takes an id and the text
  (`cli/tests.rs::session_send_takes_an_id_and_the_text_to_send`); and
  `session logs` reads the snapshot and follows the console stream
  (`commands/console.rs::a_transcript_log_uses_its_snapshot_for_table_and_json_output`,
  `::a_followed_log_uses_the_console_event_stream`).

## Sources

`crates/ariadne-cli/src/cli.rs`, `crates/ariadne-cli/src/commands/`,
`crates/ariadne-cli/src/error.rs`, `crates/ariadne-cli/src/complete.rs`,
`crates/ariadne-cli/src/output/`.
