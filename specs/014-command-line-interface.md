---
id: command-line-interface
status: current
updated: 2026-09-10
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
  - crates/ariadne-cli/src/commands/events.rs
  - crates/ariadne-cli/src/commands/task.rs
  - crates/ariadne-cli/src/output/table.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/src/commands/setup.rs
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

1. Every user-facing action exists both here and in the desktop app: neither
   surface is a subset of the other.
2. The tree is one verb per action, grouped by entity — `daemon`, `agent`,
   `models`, `skill`, `repo`, `goal`, `task`, `session`, `events`,
   `attention`, `doctor`, `completions`, plus the hidden plumbing the agents
   use (`mcp serve`, `agent-event`).
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
   `--author`/`--reviewer` slot must be written, a bare agent CLI names no
   model, and `task update --model` refuses `default` — only `--effort`
   still takes that word. A missing model, a model naming no agent, a model
   on an agent that is no CLI, an effort with no meaning and a reviewer
   naming no real agent are usage errors, refused before anything is sent.
   Completions offer neither a bare CLI nor `default` as a model.
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
12. `ariadne events` prints the daemon's own gist of an agent event's payload
    as the detail: `<agent kind> · <summary>` — the same `summary` the
    daemon builds onto the event's DTO (012), whichever CLI reported it.
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
19. Each `doctor` section is a shared output table: its check, verdict and
    detail columns fit the terminal, and `--no-trunc` prints their cells whole.
20. `task ls`, `goal ls`, `session ls` and `attention` take `--watch`: the
    table is redrawn whole on every event the command cares about, coalesced
    over a short settle window so one change is one redraw, until Ctrl-C.
    Every filter the command takes still narrows what a redraw shows, the
    redraw escapes only reach a real terminal, and `--watch` is advertised
    only on these four commands.
21. A screen of several tables is fitted once, across every group of rows:
    `ariadne attention` prints a section per goal, and a column is the same
    width under every heading — on a `--watch` redraw too. A `--columns`
    naming a column the table does not have is refused once, before any of
    the screen is printed.
22. A heading is one style everywhere: a section heading and the column header
    of a table are both bold and uppercase. `-q` prints the first cell of
    every row of every group, and nothing else — and, like every other `-q`
    listing, it reads no `--columns` and so refuses none.
23. Human mutation output is one styled line. Quiet mutation output is only
    the affected id. Inspect keys use lowercase space-separated words. A row's
    subject column is `title`, except the agent CLI kind remains `agent`.
    Boolean columns use the shared `yes_no` wording. Every empty listing states
    what is empty, then gives the next command when one exists.
24. `ariadne setup codex-hooks` renders the command and events it reports
    through the same key/value block every inspect command uses, rather than
    hand-picked spacing, and its confirmation prompt goes to stderr with every
    other prompt, so a piped stdout carries only that block.
25. `ariadne session discover` lists sessions found in the supported CLI
    stores but not started by Ariadne. `ariadne session adopt` assigns one to
    a ready task through the same REST surface (020).

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
- Model and effort misuse is a usage error
  (`::a_model_naming_no_agent_is_a_usage_error`,
  `::a_model_on_an_agent_that_is_no_cli_is_a_usage_error`,
  `::an_effort_that_says_nothing_is_a_usage_error`,
  `::a_reviewer_that_names_no_real_agent_is_a_usage_error`), and so is a line
  with no model (`::a_line_with_no_model_is_a_usage_error`).
- Completion offers no bare CLI and no `default` for a model
  (`complete.rs::the_curated_fallback_offers_no_bare_cli_and_no_default`).
- `ariadne events` prints the daemon's summary in an agent event's detail
  (`commands/events.rs::an_agent_event_reads_the_same_recorded_as_it_does_live`).
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
- `doctor` reports every agent kind, the tools a session and a published task
  need, and a worktree root it cannot write
  (`doctor.rs::every_agent_kind_is_reported`,
  `::the_tools_a_session_and_a_published_task_need_are_reported`,
  `::a_worktree_root_the_daemon_cannot_write_is_reported_as_such`), with
  details truncated to a narrow terminal unless `--no-trunc` asks for them
  whole (`commands/doctor.rs::a_narrow_terminal_truncates_doctor_details`,
  `::no_trunc_keeps_doctor_details_and_columns_whole`).
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
  `models.rs::the_bands_drop_before_efforts_and_description_do`,
  `::a_row_carries_the_bands_and_stars_the_default_effort`).
- `setup codex-hooks` aligns its command and events values through the shared
  key/value block
  (`setup.rs::the_hook_block_aligns_command_and_events_through_kv_block`).

## Sources

`crates/ariadne-cli/src/cli.rs`, `crates/ariadne-cli/src/commands/`,
`crates/ariadne-cli/src/error.rs`, `crates/ariadne-cli/src/complete.rs`,
`crates/ariadne-cli/src/output/`.
