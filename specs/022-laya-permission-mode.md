---
id: laya-permission-mode
status: current
updated: 2026-09-26
areas: [core, api, store, daemon]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/laya.rs
  - crates/ariadne-daemon/tests/it/laya_decisions.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-console/src/tui/picker.rs
  - crates/ariadne-daemon/src/laya/python.rs
  - crates/ariadne-daemon/src/laya/install.rs
  - crates/ariadne-daemon/src/laya/schedule.rs
  - crates/ariadne-daemon/src/http/permissions.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-store/tests/store.rs
---

# The `ai` permission mode and Laya

The fourth permission mode. In `ai`, a local model answers the ACP permission
requests of every session in the repository, instead of a person answering
them or a rule approving them.

That model is Laya, a Python package. Ariadne installs it: a wheel of the
package's own GitHub release, into a virtual environment under the Ariadne
home, and the checkpoints it decides with from Hugging Face. The install takes
minutes and gigabytes, so it is a background task, and everything a client
reads of it is one settings row.

## Scope

In: the `ai` mode as a value, the Laya settings, the Python check, the
install, and the three endpoints over them.

Out: how the four modes answer a request (021, rule 9), what a repository is
(002), and the wire shape every endpoint here keeps (012).

## Behavior

1. `PermissionMode` has a fourth value, `ai`, spelled `ai` on the wire, in
   the store and on the command line. It is a repository's, set and changed
   exactly as the other three are (002, rule 12).
2. Laya is the daemon's, not a repository's: one settings row, one install,
   one server. A repository chooses `ai`; this says whether there is a Laya to
   answer with.
3. The settings are `enabled`, `checkpoints`, `threshold` and `schedule`. A
   fresh daemon is off, on the English checkpoint, at a threshold of 0.8, with
   no daily refresh. `checkpoints` is `english` — 843 MB — or `all`, which
   adds the `multilingual` and `typed-decisions` subfolders and makes 2.4 GB.
   `threshold` is how sure Laya has to be before its answer is taken, 0 to 1.
   `schedule` is `HH:MM` in 24-hour local time, or nothing.
4. The state of the install is `disabled`, `installing`, `ready` or `failed`,
   and beside it are the release on disk, the release the last download named,
   whether the checkpoints are there, when the last install ended well, and
   why the last one failed. All of it is in the store, so it outlives the
   install and the daemon that ran it.
5. The Python check finds the interpreter the install would run: the
   `python_bin` config key where one is set — a path taken as it stands, a
   bare name looked up on the daemon's `PATH` — else `python3` on that same
   `PATH`. It is asked `--version`, and `ok` is true for 3.10 or newer.
   Anything that cannot be found, will not answer, or answers with no version
   is `ok: false`.
6. Turning Laya on with `ok: false` is refused with 409 `python_unavailable`,
   and writes nothing: Laya installs PyTorch into a virtual environment of
   that interpreter, and 3.9 would fail at the end of gigabytes rather than
   the start.
7. An install runs as one background task, and one at a time. It reads the
   release document at the `laya_release_url` config key — by default the
   latest release of the package's repository — under
   `Timeouts::laya_release_download`, 30 s, and takes its `tag_name` and the
   one `.whl` asset on it. It creates `<home>/laya/venv` with `python -m venv`
   where there is none, installs `laya[serve] @ <wheel url>` into it with
   pip, and downloads the checkpoints the settings name with
   `from laya import Router; Router().preload([…])` under
   `HF_HOME=<home>/laya/hf`.
8. An install that ends well writes `installed_release`, `latest_release`,
   `weights_present`, `state = ready` and `last_refresh_at`, and clears
   `last_error`. One that fails at any step writes `state = failed` and
   `last_error`, and leaves the install before it on disk: the files of a Laya
   that worked yesterday are worth more than none.
9. Every change of the status is published as the domain event
   `laya_updated`, carrying the whole `LayaStatusDto` (012, rule 4). Nothing
   waits on an install: the write that starts one answers `installing`, and
   the event says how it ended.
10. `GET /v1/permissions/laya` answers the settings, the state and the
    interpreter probed afresh, as a `LayaStatusDto`.
11. `PUT /v1/permissions/laya` takes an `UpdateLayaRequest`; an absent field
    stays as it is. A `schedule` of `null` turns the daily refresh off, and
    an absent one keeps it. A `threshold` outside 0..=1 and a `schedule` that
    is not `HH:MM` are refused with 422 and the code `invalid_request`, and
    write nothing. Turning Laya on starts an install and answers at once with
    `state = installing`. Turning it off writes `state = disabled` and keeps
    every file, so turning it back on costs the release check alone.
    Turning it off while an install runs does not stop that install. The
    install's end still writes its release, its weights and its error, but
    never its state: every state an install writes lands only on a row that is
    still enabled, in the same statement that checks it, so Laya stays
    `disabled`. Turning Laya on again while that install still runs answers
    `installing` again, and the install's end is the state it settles on.
    That `installing` is guarded the same way: a turn-off that lands after
    the write that turned Laya on and before it keeps its `disabled`, so no
    order of the two requests leaves a Laya that is off at `installing`.
12. `POST /v1/permissions/laya/refresh` runs the install again on the
    settings as they stand, and answers 202 with `state = installing`. It is
    refused with 409 `laya_disabled` while Laya is off, and 409 `laya_busy`
    while an install is running.
    The daily refresh is the same install, run by the daemon itself. It reads
    the local clock every `Timeouts::laya_schedule_poll`, 30 s, and starts an
    install once each local date when the `schedule` minute has passed. It
    runs nothing while Laya is off, without a schedule, or while an install
    runs. A daemon that starts after the minute catches up once that day. A minute a
    daylight-saving change skips is no time on that day.
13. `POST /v1/repositories` and `PUT /v1/repositories/{id}` refuse
    `permission_mode = ai` while Laya is off, with 409 `laya_disabled`. The
    refusal is where the mode is set, rather than at the first permission
    request an hour into an agent's work.
14. `GET /v1/doctor` carries the same `PythonDto` the status does. The
    interpreter is reported apart from the tools (012, rule 19) because the
    question it answers is not whether it is there but whether it is new
    enough.
15. `python_bin` and `laya_release_url` are keys of `<home>/config.toml`,
    listed by `ariadned --help` and read by `--check-config`. The test seams
    `laya_installer` — a command the daemon runs in place of the venv, the
    wheel and the checkpoints, with `LAYA_HOME`, `LAYA_CHECKPOINTS`,
    `LAYA_WHEEL_URL` and `LAYA_RELEASE` in its environment, its exit status
    deciding the install and its stderr becoming `last_error` — and
    `laya_endpoint` are settings of the daemon alone. Neither is a key of the
    user's config, and a file naming one is refused like any other unknown
    key.
16. Where the Laya server answers is `laya_endpoint` where it is configured,
    else whatever the server last reported, else nothing. Laya is live — what
    a decision needs — only while it is enabled and something is serving it.

## Server

17. While Laya is enabled and its install is ready, the daemon runs
   `<home>/laya/venv/bin/laya-serve` as its child, in a process group of its
   own. It binds an available loopback port, preloads the selected checkpoints
   from `<home>/laya/hf`, and never listens beyond the local machine. The
   server runs under a `/bin/sh` guard whose stdin is a pipe only the daemon
   writes to. When that pipe closes, however the daemon ended (`kill -9`
   included), the guard kills the whole process group, so no server outlives
   its daemon. When the server exits on its own, the guard exits with its
   status.
18. The daemon waits up to `Timeouts::laya_serve_start` for `GET /health`
   to answer with a success. It then publishes the loopback endpoint in
   `laya_updated`. On disable, refresh, shutdown, or an unsuccessful health
   wait it clears that endpoint and kills the whole process group.
19. An unexpectedly exited server clears its endpoint and restarts with a
   capped 1, 2, 4 … 60 second backoff. A disabled Laya is not restarted.

## Schedule

20. A schedule is checked every `Timeouts::laya_schedule_poll`. Once each
   local date, when enabled Laya reaches its configured `HH:MM`, it starts a
   refresh unless one is already installing. The completed date is stored, so
   a daemon started after that time catches up once that day, but subsequent
   ticks do not repeat it. A changed schedule applies at the next tick.

## Decisions

1. In `ai`, Laya decides before a learned approval is read. A request made
   while a launched server is still loading its weights waits until that
   server is healthy or given up on (at most `Timeouts::laya_serve_start`),
   so the requests of a freshly started daemon are still Laya's. The daemon
   then posts to `{endpoint}/v1/systemone` and waits at most
   `Timeouts::laya_decision`, five seconds by default.
2. The state carries the tool call's title as `tool`, its `kind`, its
   `rawInput` as compact JSON cut to 2,000 characters, the repository path,
   and the option names joined by `, `.
3. The one choice question is named `decision`. Its instructions are “Can
   this coding-agent tool call run without a person's review?”. Its `allow`
   criterion is “reading files, searching, listing, building, running tests,
   editing files inside the working tree, git commands that do not delete
   branches or force-push”. Its `review` criterion is “deleting outside the
   working tree, force pushes, package installs, network writes, credentials
   or secrets, changes to system configuration, anything unclear”.
4. The answer is read from `answers.decision`: its `choice` is the label, and
   its confidence is the calibrated `answer_confidence`, or, when that is
   absent, the chosen label's entry in `probabilities`. Laya's uncalibrated
   `confidence` is never compared with the threshold. An `allow` whose
   confidence is at least the configured threshold selects the allowing
   option. A request without an allowing option is not a
   confident allow.
5. Every other answer follows `learn` (021, rule 9): a matching learned
   approval is selected, otherwise the console is asked, and its allowing
   answer is remembered. Laya is unavailable, and a warning is logged, when
   `live()` is absent, its call fails or times out, or its answer is malformed.
6. A Laya allow is never learned. Only an allowing console answer writes the
   learned table.
7. `permission.replied` carries `decided_by`: `laya`, `learned`, `console`,
   or `auto`. A Laya reply also carries its `label` and `confidence`; these
   are null for every other decider. The console renders a Laya allow as
   `allowed by Laya (0.94)`.

## Acceptance criteria

- A fresh daemon is off, on English, at 0.8, with no schedule, and reports
  the interpreter it probed
  (`laya.rs::the_settings_start_at_the_defaults_with_the_interpreter_probed`).
- A version is read out of what an interpreter prints, and the cut is at 3.10
  (`laya/python.rs::tests::the_version_is_read_from_the_line_and_cut_at_three_ten`);
  an interpreter that is not there is reported as missing
  (`::an_interpreter_that_is_not_there_is_reported_as_missing`).
- Turning Laya on against Python 3.9 is refused with `python_unavailable` and
  changes nothing
  (`laya.rs::turning_laya_on_without_a_new_enough_python_is_refused_and_changes_nothing`).
- Turning it on against 3.12 answers `installing`, says so on the stream, and
  settles on `ready` with the release tag, the weights and the refresh time;
  the installer saw `english` and the wheel of the release document
  (`laya.rs::turning_laya_on_starts_the_install_and_reports_it_ready`).
- The release document gives the tag and the wheel, and one carrying neither
  is refused where it is read
  (`laya/install.rs::tests::the_release_document_gives_the_tag_and_the_wheel`).
- An install that fails leaves Laya on and carries the installer's own words
  (`laya.rs::an_install_that_fails_keeps_laya_on_and_says_why`).
- `checkpoints`, `threshold` and `schedule` are kept and read back by the
  next daemon; 1.5 and `25:00` are refused with 422; `null` turns the
  schedule off and an absent one keeps it
  (`laya.rs::the_settings_are_validated_and_survive_a_daemon_restart`), and a
  schedule is two digits, a colon and two digits
  (`http/permissions.rs::tests::a_schedule_is_two_digits_a_colon_and_two_digits`).
- Refresh is refused with `laya_disabled` while off and `laya_busy` while
  installing, and runs the installer again on the checkpoints set now
  (`laya.rs::refresh_is_refused_while_laya_is_off_or_busy_and_reruns_the_install`).
- Turning Laya off keeps the release and the weights
  (`laya.rs::turning_laya_off_keeps_the_files_it_installed`).
- Turning Laya off while an install runs keeps it off when that install
  ends, with the release written down
  (`laya.rs::turning_laya_off_during_an_install_keeps_it_off_when_the_install_ends`);
  turning it on again meanwhile answers `installing` and settles on the
  install's end
  (`::turning_laya_back_on_during_its_install_reports_that_install`); a
  turn-off that lands before that `installing` keeps Laya off through the
  install's end
  (`::a_turn_off_that_lands_before_the_rejoin_keeps_laya_off`); and a
  state written only while enabled leaves a row turned off at `disabled`
  (`store.rs::the_laya_settings_are_one_row_that_takes_partial_writes`).
- The daily refresh starts once per local date, catches up after daemon
  startup, and skips a disabled or busy Laya
  (`laya_server.rs::the_schedule_refreshes_once_per_local_day`).
- A repository is refused `ai` while Laya is off, on registration and on an
  edit, and takes it once Laya is on
  (`laya.rs::a_repository_takes_the_ai_mode_only_once_laya_is_on`), and the
  store round-trips the mode
  (`store.rs::a_repository_takes_the_ai_permission_mode`).
- The configured endpoint wins over the one a server reports, and nothing is
  live while Laya is off
  (`laya.rs::the_endpoint_is_the_configured_one_and_live_needs_laya_on`).
- A ready enabled Laya starts its local server with its selected checkpoints,
  reports the endpoint, stops it on disable and shutdown, restarts it after a
  refresh, a daemon restart and an unexpected exit, and leaves no child after
  shutdown (`laya_server.rs::a_ready_enabled_laya_starts_after_a_daemon_restart`).
  A server that misses its health deadline has no endpoint
  (`laya_server.rs::a_server_that_never_answers_health_is_not_live`).
- The guard kills the server once the daemon's end of its pipe closes, and
  exits with the server's status when the server exits on its own
  (`laya/server.rs::tests::the_server_dies_when_the_daemon_end_of_its_pipe_closes`,
  `::the_guard_exits_with_the_server_status`).
- A due schedule refreshes once per local date, catches up after startup, and
  does nothing while disabled (`laya_server.rs::the_schedule_refreshes_once_per_local_day`).
- The three paths, the five schemas, the nullable schedule, the doctor's
  `python` and the event kind are in the OpenAPI document
  (`laya.rs::the_endpoints_the_schemas_and_the_event_are_in_the_openapi_document`),
  and the doctor reports the interpreter apart from the tools
  (`::the_doctor_reports_the_interpreter_laya_needs`).
- The settings are one row taking partial writes, and they survive a store
  reopen
  (`store.rs::the_laya_settings_are_one_row_that_takes_partial_writes`).
- `python_bin` and `laya_release_url` are read from `config.toml`, and the
  two test seams are not keys of it
  (`config.rs::tests::the_laya_keys_a_user_may_set_are_read_and_the_test_seams_are_not`).
- A confident allow selects the allowing option, records Laya and its
  confidence, raises no attention, and sends the request state to Laya
  (`laya_decisions.rs::a_confident_allow_runs_at_once_and_reports_laya`).
- The confidence gated is Laya's calibrated `answer_confidence`, not its
  entropy `confidence`, and an answer without it is gated on the chosen
  label's probability
  (`laya_decisions.rs::a_confident_allow_runs_at_once_and_reports_laya`,
  `::an_answer_without_its_calibrated_confidence_is_gated_on_its_probability`).
- An uncertain allow asks the console, remembers its approval, and still asks
  Laya before selecting that learned approval next time
  (`laya_decisions.rs::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval`).
- A confident review asks the console
  (`laya_decisions.rs::a_review_answer_waits_for_the_console`).
- A confident allow without an allowing option asks the console
  (`laya_decisions.rs::an_allow_without_an_allowing_option_waits_for_the_console`).
- A request made while the server loads waits for it, and Laya decides it
  (`laya_decisions.rs::a_request_made_while_the_server_loads_waits_for_it`).
- A stopped or timed-out Laya warns and asks the console
  (`laya_decisions.rs::a_stopped_laya_warns_and_waits_for_the_console`,
  `::a_laya_timeout_waits_for_the_console`).
- A malformed answer warns and asks the console
  (`laya_decisions.rs::a_malformed_answer_warns_and_waits_for_the_console`).
- A Laya disabled after the repository chose `ai` asks the console
  (`laya_decisions.rs::a_disabled_laya_waits_for_the_console`).
- The unchanged `auto`, `ask`, and `learn` paths name their decider
  (`acp_runtime.rs::auto_approves_a_permission_request_with_the_allowing_option`,
  `acp_console.rs::ask_raises_attention_and_a_console_answer_unblocks_the_turn`,
  `::learn_remembers_an_approval_per_repository_across_a_daemon_restart`).
- A Laya reply renders its decider and confidence
  (`ariadne-console::tui::picker::tests::a_laya_answer_names_laya_and_its_confidence`).

## Sources

`crates/ariadne-core/src/lib.rs`,
`crates/ariadne-api/src/permissions.rs`,
`crates/ariadne-api/src/stream.rs`,
`crates/ariadne-api/src/doctor.rs`,
`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/src/laya.rs`,
`crates/ariadne-daemon/src/laya/`,
`crates/ariadne-daemon/src/main.rs`,
`crates/ariadne-daemon/tests/it/laya_decisions.rs`,
`crates/ariadne-daemon/src/http/permissions.rs`,
`crates/ariadne-daemon/src/http/repositories.rs`,
`crates/ariadne-daemon/src/http/doctor.rs`,
`crates/ariadne-daemon/src/config.rs`,
`crates/ariadne-daemon/src/acp.rs`.
