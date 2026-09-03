---
id: command-line-interface
status: current
updated: 2026-09-04
areas: [cli]
commits: [3dcba5f1, e94647fd, 3cd70453, 9f7fa36b, 1a862dfe]
tests:
  - crates/ariadne-cli/src/cli/tests.rs
  - crates/ariadne-cli/src/error.rs
  - crates/ariadne-cli/src/complete.rs
  - crates/ariadne-daemon/tests/doctor.rs
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
   `models`, `profile`, `repo`, `goal`, `task`, `session`, `events`,
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
7. A model can be chosen for every agent on the line, with an effort beside
   it; a model naming no agent, a model on an agent that is no CLI, an effort
   with no meaning and a reviewer naming no real agent are usage errors,
   refused before anything is sent.
8. A failure prints `error: <sentence>` and nothing else: no `Caused by:`
   block, no transport detail, no repeated envelope. `--format json` prints
   the daemon's envelope instead, so a script keeps the status and code the
   human line drops.
9. The exit code says what kind of failure it was, and every kind has one of
   its own; it is documented in `ariadne --help`.
10. Completions are generated for bash and zsh and complete against live data:
    candidates newest first, live sessions before ended ones when attaching,
    the efforts an entry lists and no others.
11. `ariadne doctor` answers why the daemon will not start — including a
    database written by a release whose migrations this one no longer ships,
    which it names along with the file to delete (016).

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
  `::a_reviewer_that_names_no_real_agent_is_a_usage_error`).
- A failure is one line, and JSON keeps the envelope
  (`error.rs::a_bare_message_is_the_whole_line`,
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
  `::a_worktree_root_the_daemon_cannot_write_is_reported_as_such`).

## Sources

`crates/ariadne-cli/src/cli.rs`, `crates/ariadne-cli/src/commands/`,
`crates/ariadne-cli/src/error.rs`, `crates/ariadne-cli/src/complete.rs`.
