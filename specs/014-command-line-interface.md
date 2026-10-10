---
id: command-line-interface
status: current
updated: 2026-10-09
areas: [cli]
commits: [3dcba5f1, e94647fd, 3cd70453, 9f7fa36b, 1a862dfe, 87fa62cf, 03f9c8b7, 29e6d84e, 1b09ac10, 7fe184e9]
tests:
  - crates/ariadne-cli/src/commands/workflow.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - crates/ariadne-cli/src/output.rs
  - crates/ariadne-cli/src/commands/models.rs
  - crates/ariadne-cli/src/commands/permissions.rs
  - crates/ariadne-client/src/lib.rs
  - crates/ariadne-cli/src/commands/session.rs
  - crates/ariadne-cli/src/commands/task.rs
  - crates/ariadne-cli/src/error.rs
  - crates/ariadne-cli/src/complete.rs
  - crates/ariadne-daemon/tests/it/doctor.rs
  - crates/ariadne-cli/src/commands/doctor.rs
  - crates/ariadne-cli/src/commands/doctor/checks.rs
  - crates/ariadne-cli/src/commands/doctor/agents.rs
  - crates/ariadne-cli/src/commands/events.rs
  - crates/ariadne-cli/src/commands/console/tui.rs
  - crates/ariadne-cli/src/output/table.rs
  - crates/ariadne-cli/src/commands/attention.rs
  - crates/ariadne-cli/src/commands/agent.rs
  - crates/ariadne-cli/src/commands/skill.rs
  - crates/ariadne-cli/src/commands/console.rs
  - crates/ariadne-cli/src/commands/stats/mod.rs
  - crates/ariadne-cli/src/commands/task/edit.rs
  - crates/ariadne-cli/src/commands/goal.rs
  - crates/ariadne-cli/src/commands/repo.rs
  - crates/ariadne-cli/src/commands/attach.rs
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

1. Every user-facing action exists both here and in the desktop app.
2. The tree is one verb per action, grouped by entity — `daemon`, `agent`,
   `models`, `skill`, `repo`, `pr` (026), `permissions`, `goal`, `task`, `session`,
   `events`, `attention`, `attach`, `stats`, `doctor`, `completions`,
   plus the one hidden command the agents use (`mcp serve`). Nothing in the tree launches
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
   `--agent` slot must be written, a bare agent names no model, and
   `--model default` is refused — only `--effort` still takes that word. A
   missing model, a model naming no agent, an effort with no meaning and an
   agent slot missing a half are usage errors, refused before anything is
   sent. Which agents exist is the daemon's registry: an agent id is sent as
   typed, and the daemon refuses one it does not hold (011).
8. The empty list is a flag of its own wherever a repeatable flag names one,
   since a repeatable flag cannot be given zero times on purpose:
   `--clear-depends-on` for a task with nothing to wait for.
9. Every judgement about a goal's work can be made from here too: the
   workflow its tasks run through (`goal create --workflow`, fixed once the
   goal is created, and the first repository's default where it is not
   given: 030), how each task is staffed (`--agent`, rule 38), and whether
   the goal is over (`goal complete`).
10. What the agents said to each other is readable from here: `task messages`
    lists the whole channel of a task, oldest first (018); `--full` prints
    each one whole, through `$PAGER`, since the table cuts a body to its
    opening.
11. `task history` is a table too: each row is one transition, `from` and
    `to` painted with the same status glyphs every other status cell carries.
12. `ariadne events` prints one line per event, `time · kind · subject ·
    detail`. An agent event's detail is the `summary` the daemon builds onto
    its DTO (012), and a recorded event reads the same as a live one. The
    snapshot it opens with is the 200 most recent recorded events, asked for
    newest first and printed oldest first, so `-f` goes on in the same
    direction. `--goal` narrows that snapshot at the daemon (012), like
    `--task` and `--session`.
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
    the affected id. Inspect keys use lowercase space-separated words.
    `ariadne goal inspect` shows its workflow and its columns (rule 40), and
    no landing and no goal branch.
    Missing session goals, tasks and seats print a dash.
    A row's subject column is `title`, except that the agent listing keeps `agent`.
    Boolean columns use the shared `yes_no` wording. Every empty listing states
    what is empty, then gives the next command when one exists.
25. `ariadne agent ls|update` lists and edits the flags each registry agent
    is launched with, keyed by its registry id. `update` takes a flag list, a
    clear, or a reset, and only one of them, and a flag value that reads like
    a flag of the CLI's own is taken as it is. `ariadne agent refresh` reprobes
    every ACP registry agent, then lists the daemon's result with each id,
    status and command, plus the reason of every rejected agent. Its JSON is
    the daemon's list unchanged.
26. `ariadne session ls` lists one filtered page of Ariadne sessions and
    outside ACP sessions. Its columns are `id`, `title`, `status`, `goal`,
    `task`, `agent`, `model`, `age` and `tokens`; `directory` is available with
    `--columns`. An outside row leaves status, goal and task empty. It sends
    `--kind`, `--agent`, `--status`, `--seat`, `--goal`, `--task`, `--attention`,
    `--dir`, `--since`, `--until`, `--search`, `--limit`, `--cursor`,
    `--refresh` and `--all` to the daemon; `--search` becomes `q`, and agent
    flags complete registry agent ids. A date activity bound is the start of
    its UTC day for `--since` and the end for `--until`. The table ends with
    `<shown> of <total> sessions` and a reusable next-page command when the
    daemon returns a cursor. `--all` follows every cursor into one table and
    cannot be combined with `--cursor`. JSON preserves the page object, while
    quiet output prints its session ids. `session discover` does not exist.
27. `ariadne attach`, `goal attach` and `task attach` open the console of the
    session an id names, revived first when it is gone. `ariadne attach` also
    takes an outside internal id: it resumes the conversation, then opens its
    console. An outside id shared by agents requires `--agent`. A finished
    task whose worktree is gone revives its session. On a terminal it is an
    inline pane (008); with stdin or stdout redirected it is the plain line
    protocol — one `kind · summary` per event, numbered permission choices,
    and one prompt per line read. `session send` posts one line to that same
    console input. `session logs` and `task logs` print readable transcript
    blocks with local timestamps, complete text, plans, tool results and
    permission answers. `--tail`, `--since` and repeatable `--kind` narrow the
    snapshot. Their `-f` forms follow the console stream and print agent chunks
    as they arrive, while JSON keeps the daemon's event objects unchanged.
30. `session inspect` shows a reported context window as `<used> / <size>`
    with the compact token spelling. It shows no context line when the agent
    has not reported one, and it never shows a cost.
30a. `session inspect` prints `title` after `id`, and `continues` after
    `internal id`. `continues` is the id of the session a `session switch`
    replaced on its seat. Each prints a dash where the DTO carries none.
31. `session new --model AGENT:MODEL [--effort E] [--dir PATH] [--attach]`
    starts a loose session (020): a new conversation with no goal or task,
    in `--dir` or the current directory, sent to the daemon as an absolute
    path. It prints the session's status line, or with `--attach` opens its
    console at once. The first prompt typed into it is its title.
31a. `session switch <id> --model AGENT:MODEL [--effort EFFORT]` switches
    the session to a new conversation on the requested pin, preserving its
    seat. It refuses `--model default` locally; `--effort default` leaves
    effort unpinned. It prints the new session id and pin, or the full DTO
    with `--format json`; daemon refusals print whole.
32. `ariadne permissions ai` manages the AI permission model behind the `ai`
    permission mode (022); `ariadne permissions` prints the group help and
    accepts no flat command. `show` is a key-value block of the whole status,
    and `--format json` prints the DTO unchanged. `enable` and `disable` turn
    it on and off, `refresh` reinstalls
    it, and each of the two that starts an install takes `--wait`: it blocks
    until `state` leaves `installing` by following `ai_permissions_updated` on
    `/v1/events/stream`, and exits 1 with `last_error` where it settles on
    `failed`. `set` changes `--threshold`, `--flavour` or `--device` (022,
    flavours and devices); at least one setting is required, and only the
    flags actually given reach the daemon. A bad `--threshold`, `--flavour`
    or `--device` is
    refused locally, in the same words the daemon would use, before anything
    is sent; the daemon's own `flavour_unsupported` refusal survives whole. An `ai_disabled` refusal — here and on `repo add|update
    --permission-mode ai` alike — carries the hint `run ariadne permissions ai
    enable`; a `python_unavailable` one carries `install Python 3.12 or 3.13,
    or set python_bin in config.toml`.
33. `ariadne permissions ai test --tool <title> [--kind <K>] --input <json>
    [--option <name>]... [--location <path>]... [--workspace <dir>]` sends one
    request to the AI permission model without selecting or recording an
    approval. `--tool` is the tool call title the model sees, not the tool
    name. It refuses invalid input JSON locally,
    prints one score line for people and the response unchanged with
    `--format json`.
34. `ariadne doctor` reports the Python interpreter the AI permission model's install would run
    on, next to the daemon's own environment (rule 19), and where the install
    itself stands. Neither ever fails the report, since `ai` is one
    permission mode among four: `python` is `ok` with the version found, or a
    warning naming the version that is too old or that none was found;
    `ai permissions` is `ok` for `disabled`, `installing` and `ready <release>`, and a
    warning for `failed: <last_error>`.
34. `ariadne permissions learned` lists, shows, removes and widens or narrows
    learned permissions. `list` shows the columns id, repository, tool, level,
    family, key, scope, target, selected, ai, created and updated; it has no
    source column. The AI column shows the label and danger to four decimal
    places, or a dash where the model did not score the row. `show` prints
    every field: the level, the family, the key, the risk tags and the scope
    among them, and the JSON fields pretty-printed, the raw input in the
    tool call included. `scope <id> <all|repository>` sends the chosen scope
    to `PUT /v1/permissions/learned/{id}` (012, rule 26) and prints the
    updated row; a scope that is neither word is refused locally, before
    anything is sent, and the daemon's own refusal of an unknown id or a bad
    scope prints whole. There is no `add` or `edit`. A repository can be an
    id or path, and every verb supports JSON.
35. `ariadne stats [work|time|spend|models|attention] [--since
    <duration|date>] [--repo <id>]` prints one stat family off
    `GET /v1/stats/<family>` (023). `ariadne stats` alone prints `work`.
    Each of the five is a listing: it takes the table flags, and
    `--format json` prints the family's DTO whole. The five print one way:
    the figures as one aligned key-value block, then each table, one blank
    line before each. `stats models` titles the group of sessions with no
    seat `LOOSE SESSIONS`.
36. `ariadne workflow` manages the workflow catalog (030) the way `ariadne
    skill` manages skills: `ls` lists every workflow, shipped and written,
    its `name` column marked `builtin` with the shared `yes_no` wording and
    its `columns` column the titles of its steps joined with ` → `; `show`
    prints the document whole, through `$PAGER` as `task messages --full`
    does, and `--format json` prints the DTO; `create --file <path>|-` and
    `update --file <path>|-` read the document from `--file`, or from stdin
    where none is named or where `-` names it, and send `{document}`; `reset`
    puts a shipped workflow back on the text Ariadne
    ships, and `rm` deletes one of the user's own, each behind the usual
    confirmation. `check --file <path>|-` posts the document to the parse
    route without saving it anywhere: it prints the columns of a document
    that parses, or the daemon's `workflow_invalid` refusal read apart from
    the rest — the line named in its `details` beside the sentence next to
    it, `line <N>: <message>` — as a usage error; every other daemon refusal
    of a workflow command prints whole, the way any other command's does.
37. A goal is run through a workflow with `goal create --workflow <name>`;
    `repo add --workflow <name>` and `repo update --workflow <name>` set a
    repository's own default, which `repo ls` and `repo inspect` show. Both
    complete the name against `GET /v1/workflows`. There is no `--landing`:
    a workflow's own gates decide how a task ends (030).
38. A task is staffed with `--agent STEP[:SKILLS]=MODEL[@EFFORT]`,
    repeatable, one per column, sent as `agents`. `SKILLS` is optional and
    comma-separated; left out, the column stages its own. `task update
    --agent` replaces the task's whole staffing, and `task update` takes no
    `--model` or `--effort` of its own: a pin belongs to a column's agent.
    There is no `--author`, `--reviewer` or `--no-reviewer`, and no help
    text names an author, a reviewer or a landing.
39. `task ls` carries a `step` column after `status`; `--step <id>` narrows
    the list to it, client-side, the way every filter the daemon's own
    listing does not know narrows elsewhere in this tree. `task inspect`
    prints the task's `workflow`, its current `step`, and one line per
    column of the goal's workflow — the column, its skills, its pin and its
    session, a column nobody has staffed yet included. `task history`
    paints the column a move left and the one it entered beside the from and
    to statuses.
40. `goal inspect` prints the goal's workflow and, one line per column, its
    rank and its gate.
41. `task attach --step <id>` opens the console of that column's agent,
    live or revived, whether or not it is the task's current column; left
    out, `task attach`, `ariadne attach` and `task logs` resolve the task's
    current column first and pick that column's agent's session, live or
    revived, however the sessions of the columns the task has left are
    listed. `--seat` takes `orchestrator`,
    `agent` or `reviewer` wherever a seat is, and `session ls --seat agent`
    lists every column session. A status filter takes the six statuses of
    001 and no other word.

## Acceptance criteria

- Each `ariadne stats <family> --format json` reads its route with the
  filters given, and `ariadne stats` alone runs `work`
  (`commands/stats/mod.rs::tests::each_family_reads_its_route_with_the_filters_given`,
  `::stats_alone_runs_work`).

- `permissions learned` provides list, show and remove commands, and `add`
  and `edit` do not parse
  (`cli/tests.rs::every_learned_permissions_verb_parses_and_add_and_edit_are_gone`).
- `list` and `show` print the level, the family, the key and the scope,
  and `show` the raw input
  (`commands::permissions::tests::learned_list_and_show_print_the_new_fields`).
- `scope` parses `all` and `repository` and refuses any other value locally
  (`cli/tests.rs::permissions_learned_scope_parses_and_refuses_a_bad_value_locally`),
  sends `{"scope": "all"}` and nothing else
  (`commands::permissions::tests::scope_sends_the_new_scope_and_nothing_else`),
  and prints the daemon's refusal of an unknown id whole
  (`::scope_prints_the_daemons_not_found_refusal`).

- The command tree is well formed and every command is classified
  (`cli/tests.rs::the_command_tree_is_well_formed`,
  `::every_command_in_the_tree_is_classified`).
- Both transcript commands advertise and parse their three filters
  (`cli/tests.rs::transcript_filters_are_pinned_on_both_log_commands`).
- The root and every group are one help-screen shape
  (`::the_root_and_every_group_are_one_help_screen_shape`), and no help screen
  leaks the endpoint (`::no_help_screen_leaks_the_endpoint_of_the_shell_it_runs_in`).
- `--format` and the listing flags are advertised exactly where they are
  honoured (`::format_is_advertised_exactly_where_it_is_honored`,
  `::the_listing_flags_are_advertised_exactly_where_they_are_honored`), and
  parse on either side (`::the_display_flags_parse_on_either_side_of_the_subcommand`).
- A status is spelled in kebab or snake, and so is every other enum a flag
  takes — `--seat` and `repo add --permission-mode` among them, `ai`
  included, while `task create` takes no permission mode; several statuses
  ride on one flag, a non-status lists the six real ones, and a retired
  status word is one of them
  (`::a_status_is_spelled_in_kebab_or_in_snake`, `::several_statuses_ride_on_one_flag`,
  `::a_status_that_is_no_spelling_of_one_lists_the_real_ones`,
  `::repo_add_permission_mode_ai_parses`).
- A model and an effort can be chosen for every agent on the line
  (`cli/tests.rs::a_model_can_be_chosen_for_every_agent_on_the_line`,
  `::an_effort_can_be_chosen_beside_every_model`).
- Model and effort misuse is a usage error
  (`::a_model_naming_no_agent_is_a_usage_error`,
  `::an_effort_that_says_nothing_is_a_usage_error`,
  `::an_agent_slot_that_names_no_real_agent_is_a_usage_error`), and so is a line
  with no model (`::a_line_with_no_model_is_a_usage_error`), while an agent id
  is the daemon's to check (`::an_agent_id_is_the_daemons_to_check`).
- `models ls` narrows to an agent and `models show` takes and refuses a model
  the way `--model` does
  (`cli/tests.rs::models_ls_takes_an_agent_to_narrow_the_catalogue`,
  `::models_show_takes_a_model_in_the_spelling_dash_dash_model_takes`).
- `models rank` takes each of the four ranks or `--clear` in place of one, and
  refuses an unknown word by name and naming neither
  (`cli/tests.rs::models_rank_takes_each_of_the_four_ranks_or_clears_it`,
  `::models_rank_refuses_an_unknown_word_and_naming_neither_rank_nor_clear`).
  Setting a rank puts the id and the rank whole, `--clear` puts a `null`
  rank, and an unknown model id keeps the daemon's own refusal
  (`models.rs::rank_puts_the_id_and_the_rank_to_the_daemon`,
  `::rank_clear_puts_a_null_rank_to_the_daemon`,
  `::rank_keeps_the_daemons_refusal_of_an_unknown_model`).
- A row's rank is its word, or a dash for a model nobody ranked
  (`models.rs::the_rank_word_names_the_rank_or_dashes_when_there_is_none`,
  `::a_row_stars_the_default_effort_and_dashes_what_is_unsaid`).
- Completion offers the efforts an entry lists and no others
  (`complete.rs::an_entry_offers_the_efforts_it_lists_and_no_others`).
- Only a terminal on both ends gets the inline console
  (`console/tui.rs::only_a_terminal_on_both_ends_gets_the_inline_console`),
  and the plain line protocol is unchanged behind it
  (`console.rs::a_console_renders_a_stub_agent_transcript_and_delivers_an_input_line`,
  `::a_permission_question_renders_and_delivers_the_selected_answer`).
- `ariadne events` prints the daemon's summary in an agent event's detail and
  spells the AI permission subject `ai permissions`
  (`commands/events.rs::an_event_reads_as_time_kind_subject_and_detail`,
  `::an_agent_event_reads_the_same_recorded_as_it_does_live`,
  `::an_ai_permissions_event_names_its_subject`), and its
  snapshot is the newest page of the listing, printed oldest first
  (`::the_snapshot_asks_for_the_newest_page_and_prints_it_oldest_first`).
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
- `doctor` reports the Python interpreter the AI permission model needs without ever failing on
  it, and reports the four states of the model install
  (`commands/doctor/agents.rs::python_never_fails_and_names_what_it_found`,
  `::ai_permissions_reports_its_four_states_and_never_fails`).
- Every `permissions ai` verb parses, the group prints help, the former flat
  commands are refused, `set` with no flag is a usage error, and a bad
  threshold, flavour or device is
  refused locally in the daemon's own words
  (`cli/tests.rs::every_permissions_verb_parses`,
  `::permissions_group_prints_help_and_refuses_the_old_flat_commands`,
  `::permissions_set_with_no_flag_is_a_usage_error`,
  `::permissions_set_refuses_a_bad_threshold_flavour_or_device_locally`).
- `permissions ai test` parses its request, rejects invalid JSON locally, and
  prints its score line and JSON response
  (`cli/tests.rs::every_permissions_verb_parses`,
  `::permissions_test_refuses_bad_json_locally`,
  `commands/permissions.rs::tests::test_prints_the_score_line_and_names_no_answer`).
- `show` renders every status field except built-in configuration, and the
  flavour and device table; `enable` sends only `{"enabled": true}`, and
  `set --flavour` sends only the flavour
  (`commands/permissions.rs::show_renders_every_field`,
  `::show_omits_the_built_in_configuration`,
  `::flavour_rows_lists_every_flavour_and_device`,
  `::enable_sends_enabled_true_and_nothing_else`,
  `::set_flavour_sends_the_flavour_alone`).
- `enable --wait` and `refresh --wait` block on the event stream until the
  install leaves `installing`, and a failed install exits with its
  `last_error`
  (`commands/permissions.rs::enable_wait_returns_once_the_stream_answers_ready`,
  `::enable_wait_fails_with_the_last_error_on_a_failed_install`).
- An `ai_disabled` refusal keeps the daemon's message and adds the command
  that answers it, on `permissions` and on `repo add|update
  --permission-mode ai` alike
  (`commands/permissions.rs::refresh_keeps_the_daemons_message_and_adds_the_hint_on_ai_disabled`,
  `ariadne-client/src/lib.rs::an_ai_refusal_carries_the_command_that_answers_it`).
- A git below the floor is a warning that names what it cannot do, and a
  version line is read down to its major and minor
  (`checks.rs::a_git_below_the_floor_is_a_warning_about_repositories_with_no_commits`,
  `::a_version_line_reads_down_to_its_major_and_minor`).
- The skill listing marks an orchestrator-only skill
  (`skill.rs::a_listing_marks_an_orchestrator_only_skill`).
- Every `workflow` verb parses and is classified
  (`cli/tests.rs::every_workflow_verb_parses_and_is_classified`); `--format`
  and the listing flags are advertised exactly where they are honoured, with
  `workflow ls` a listing and `workflow show` paged, by the same checks as
  every other command
  (`cli/tests.rs::format_is_advertised_exactly_where_it_is_honored`,
  `::the_listing_flags_are_advertised_exactly_where_they_are_honored`).
- `workflow ls` joins a workflow's columns and marks a built-in; `create` and
  `update` read a file and stdin and send `{document}`; `check` reads a
  `workflow_invalid` refusal's line out of its `details` and exits as a
  usage error, and reads back the columns of a document that parses
  (`commands/workflow.rs::the_columns_label_joins_their_titles`,
  `::a_listing_marks_a_built_in_with_the_shared_boolean_wording`,
  `::create_and_update_send_the_document_whole_and_nothing_else`,
  `::check_prints_the_line_of_a_refusal_as_a_usage_error`,
  `::check_reads_back_the_columns_of_a_good_document`).
- `goal create --workflow` sends the name, and `--landing` does not parse
  (`cli/tests.rs::goal_create_workflow_sends_the_name_and_landing_is_gone`).
  `repo add --workflow` and `repo update --workflow` send `default_workflow`,
  and `repo ls` and `repo inspect` show it
  (`commands/repo.rs::repo_add_and_update_take_the_workflow_flag`).
- `--agent STEP[:SKILLS]=MODEL[@EFFORT]` stages one workflow column; the
  skills half is optional, and left out the column stages its own. `task
  create` sends one `agents` entry per `--agent`; `task update --agent`
  replaces the whole staffing, and `--author`, `--reviewer`,
  `--no-reviewer`, `task update --model` and `task update --effort` do not
  parse
  (`cli/tests.rs::an_agent_slot_names_its_column_skills_model_and_effort`,
  `commands/task/edit.rs::an_agent_slot_names_its_column_skills_model_and_effort`,
  `::only_the_flags_that_were_given_reach_the_daemon`).
- No help screen names an author, a reviewer or a landing
  (`cli/tests.rs::no_help_screen_names_an_author_a_reviewer_or_a_landing`).
- `task ls` carries a `step` column after `status`, and `--step` narrows the
  list to it client-side; `task inspect` prints a stepped task's `workflow`,
  `step`, and one line per column — its skills, its pin and its session,
  unstaffed columns included; `task history` paints the column a move left
  and the one it entered beside the statuses
  (`commands/task.rs::step_narrows_the_list_to_the_named_column`,
  `::a_task_lists_its_workflow_and_every_columns_agent`,
  `::a_history_row_paints_the_columns_a_move_crossed`).
- `goal inspect` prints the workflow and, one line per column, its rank and
  its gate (`commands/goal.rs::workflow_columns_reads_the_rank_and_the_gate`).
- `task attach --step` resolves the named column's session whether or not it
  is the task's current one; without it, attach, logs and the revive pick
  the current column's session with the sessions of two columns live and
  the left column listed first; `--seat` parses the three seats and no other
  word
  (`commands/attach.rs::resolve_step_finds_the_named_columns_session`,
  `::resolve_live_and_revive_pick_the_current_columns_session`,
  `cli/tests.rs::a_filter_takes_only_the_values_the_daemon_knows`).
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
- `ariadne attention` reads `GET /v1/sessions` as a page object, asking for
  the Ariadne kind alone and following every `next_cursor` until the last
  page, so a session the first page left off still reaches the board
  (`attention.rs::the_board_renders_against_a_paged_sessions_response`,
  `::every_page_of_ariadne_sessions_is_fetched`).
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
- A loose session prints dashes for absent goal, task and seat fields
  (`session.rs::a_loose_session_prints_dashes_for_missing_fields`).
- `session inspect` shows a reported context window with compact token counts
  and omits an unreported one
  (`session.rs::the_inspect_block_shows_the_reported_context_window`,
  `::the_inspect_block_hides_an_unreported_context_window`).
- `session inspect` prints `title` and `continues`, dashed where the DTO
  carries neither
  (`session.rs::a_switched_session_shows_its_title_and_what_it_continues`,
  `::a_session_with_neither_shows_a_dash_for_title_and_continues`).
- `agent update` takes flags, a clear or a reset but only one, and keeps a
  flag that looks like a flag as it is
  (`cli/tests.rs::updating_an_agent_takes_flags_or_clear_or_reset_but_only_one`,
  `::an_agent_flag_that_looks_like_a_flag_is_taken_as_it_is`), and its
  listing keeps the `agent` column name
  (`agent.rs::the_agent_keeps_the_agent_column_name`). `agent refresh` is in
  the command tree, calls its reprobe endpoint once, and preserves a rejected
  agent's reason (`cli/tests.rs::refresh_reprobes_every_agent`,
  `agent.rs::refresh_calls_the_reprobe_endpoint_once`,
  `::a_rejected_agent_keeps_its_reason_in_the_refresh_row`).
- `session ls` sends every combined-session filter and page flag with UTC date
  bounds, follows all pages without repeating a session, prints the count and
  reusable next-page command only when one exists, and refuses `--all` with
  `--cursor` (`session.rs::every_session_flag_reaches_its_query_parameter`,
  `::a_date_is_the_utc_day_boundary_for_session_listing`,
  `::all_fetches_every_page_and_keeps_each_session_once`,
  `::a_next_cursor_prints_the_session_command_for_the_next_page`,
  `::the_session_table_has_the_unified_columns_and_empty_outside_fields`,
  `::an_outside_row_shows_its_model_and_token_usage`,
  `::an_outside_row_without_model_or_usage_keeps_both_cells_empty`,
  `::an_ariadne_row_keeps_its_agent_model_and_token_columns`,
  `::columns_select_the_model_by_header_name`,
  `cli/tests.rs::session_ls_takes_filters_pages_refresh_and_all`,
  `::session_ls_all_and_cursor_are_exclusive`). Outside ids resume through
  the resume endpoint and a shared id requires `--agent`
  (`attach.rs::an_outside_id_resumes_through_the_resume_endpoint`,
  `::an_outside_id_shared_by_agents_requires_an_agent_flag`).
- The console renders a stub-agent transcript and submits typed input, and
  its permission question submits the selected option
  (`commands/console.rs::a_console_renders_a_stub_agent_transcript_and_delivers_an_input_line`,
  `::a_permission_question_renders_and_delivers_the_selected_answer`);
  `session send` takes an id and the text
  (`cli/tests.rs::session_send_takes_an_id_and_the_text_to_send`);
  `session new` starts a loose session on a model, in the current directory
  or the one `--dir` names, sent as an absolute path
  (`commands/session.rs::new_starts_a_session_in_the_directory_as_an_absolute_path`);
  `session switch` posts its pin and prints the returned session, while
  `default` is refused locally
  (`session.rs::switch_posts_the_pin_and_returns_the_new_session`,
  `cli/tests.rs::session_switch_requires_a_model_and_accepts_effort_default`); and
  `session logs` reads the snapshot and follows the console stream
  (`commands/console.rs::a_transcript_log_uses_its_snapshot_for_table_and_json_output`,
  `::a_followed_log_uses_the_console_event_stream`).

## Sources

`crates/ariadne-cli/src/cli.rs`, `crates/ariadne-cli/src/commands/`,
`crates/ariadne-cli/src/error.rs`, `crates/ariadne-cli/src/complete.rs`,
`crates/ariadne-cli/src/output/`, `crates/ariadne-client/src/lib.rs`.
