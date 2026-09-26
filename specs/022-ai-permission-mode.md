---
id: ai-permission-mode
status: current
updated: 2026-09-26
areas: [core, api, store, daemon]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/ai_permissions.rs
  - crates/ariadne-daemon/tests/it/ai_permissions_decisions.rs
  - crates/ariadne-daemon/tests/it/ai_permissions_server.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-console/src/tui/picker.rs
  - crates/ariadne-client/src/lib.rs
  - crates/ariadne-daemon/src/ai_permissions/python.rs
  - crates/ariadne-daemon/src/ai_permissions/install.rs
  - crates/ariadne-daemon/src/ai_permissions/server.rs
  - crates/ariadne-daemon/src/http/permissions.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-store/tests/store.rs
---

# The `ai` permission mode

The fourth permission mode. In `ai`, a local model answers the ACP permission
requests of every session in the repository, instead of a person answering
them or a rule approving them.

That model is the AI permission model. The daemon installs its pinned package
from Git into a virtual environment under the Ariadne home. It downloads the
pinned Hub adapter and base model from Hugging Face. The install takes minutes
and gigabytes, so it is a background task, and everything a client reads of it
is one settings row.

## Scope

In: the `ai` mode as a value, the model's settings and prompts, the Python
check, the install, the server, the decision, and the three endpoints over
them.

Out: how the four modes answer a request (021, rule 9), what a repository is
(002), and the wire shape every endpoint here keeps (012).

## Behavior

1. `PermissionMode` has a fourth value, `ai`, spelled `ai` on the wire, in
   the store and on the command line. It is a repository's, set and changed
   exactly as the other three are (002, rule 12).
2. The model is the daemon's, not a repository's: one settings row
   (`ai_permission_settings`), one install, one server. A repository chooses
   `ai`; this says whether there is a model to answer with.
3. The settings are `enabled`, `threshold`, and `schedule`. The
   Kev run and the decision prompts are built in.
   `threshold` is how sure
   the model has to be before its answer is taken, 0 to 1. `schedule` is
   `HH:MM` in 24-hour local time, or nothing.
4. The state of the install is `disabled`, `installing`, `ready` or `failed`,
   and beside it are the pin on disk, the pin the last install used,
   whether the checkpoints are there, when the last install ended well, and
   why the last one failed. All of it is in the store, so it outlives the
   install and the daemon that ran it.
5. The Python check finds the interpreter the install would run: the
   `python_bin` config key where one is set — a path taken as it stands, a
   bare name looked up on the daemon's `PATH` — else it tries `python3.13`,
   then `python3.12`, then `python3` on that same `PATH`. It is asked
   `--version`, and `ok` is true only for 3.12 or 3.13.
   Anything that cannot be found, will not answer, or answers with no version
   is `ok: false`.
6. Turning the model on with `ok: false` is refused with 409
   `python_unavailable`, and writes nothing: the install puts PyTorch into a
   virtual environment of that interpreter, and an unsupported version would fail at the end of
   gigabytes rather than the start.
7. An install runs as one background task, and one at a time. It creates
   `<home>/ai-permissions/venv` with a supported Python interpreter. It rebuilds
   a venv made by another interpreter, while retaining `<home>/ai-permissions/hf`.
   It installs `kev[serve]` from git at commit `f1535963cea021439370c23127bc970b6788e730`,
   downloads adapter `jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`,
   and downloads base `Qwen/Qwen3.5-4B-Base@1001bb4d826a52d1f399e183466143f4da7b741b`
   under `HF_HOME=<home>/ai-permissions/hf`.
8. An install that ends well writes `installed_release`, `latest_release` as
   `kev@f1535963 jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`,
   `weights_present`, `state = ready` and `last_refresh_at`, and clears
   `last_error`. One that fails at any step writes `state = failed` and
   `last_error`, and leaves the install before it on disk: the files of a
   model that worked yesterday are worth more than none.
9. Every change of the status is published as the domain event
   `ai_permissions_updated`, carrying the whole `AiPermissionsStatusDto` (012,
   rule 25). Nothing waits on an install: the write that starts one answers
   `installing`, and the event says how it ended.
10. `GET /v1/permissions/ai` answers the settings, the state and the
    interpreter probed afresh, as an
    `AiPermissionsStatusDto`.
11. `PUT /v1/permissions/ai` takes an `UpdateAiPermissionsRequest`; an absent
    field stays as it is. A `schedule` of `null` turns the daily refresh off,
    and an absent one keeps it. A `threshold` outside 0..=1 or a `schedule`
    that is not `HH:MM` is refused with 422 and the code `invalid_request`,
    and writes nothing. Turning the model
    on starts an install and answers at once with `state = installing`.
    Turning it off writes `state = disabled` and keeps every file, so turning
    it back on repairs the pinned package and weights.
    Turning it off while an install runs does not stop that install. The
    install's end still writes its release, its weights and its error, but
    never its state: every state an install writes lands only on a row that is
    still enabled, in the same statement that checks it, so the model stays
    `disabled`. Turning it on again while that install still runs answers
    `installing` again, and the install's end is the state it settles on.
    That `installing` is guarded the same way: a turn-off that lands after
    the write that turned the model on and before it keeps its `disabled`, so
    no order of the two requests leaves a model that is off at `installing`.
12. `POST /v1/permissions/ai/refresh` runs the install again on the settings
    as they stand, and answers 202 with `state = installing`. It is refused
    with 409 `ai_disabled` while the model is off, and 409 `ai_busy` while an
    install is running. A manual or scheduled refresh reinstalls the same pins to repair
    the installation; it never upgrades them.
    The daily refresh is the same install, run by the daemon itself. It reads
    the local clock every `Timeouts::ai_permissions_schedule_poll`, 30 s, and
    starts an install once each local date when the `schedule` minute has
    passed. It runs nothing while the model is off, without a schedule, or
    while an install runs. A daemon that starts after the minute catches up
    once that day. A minute a daylight-saving change skips is no time on that
    day.
13. `POST /v1/repositories` and `PUT /v1/repositories/{id}` refuse
    `permission_mode = ai` while the model is off, with 409 `ai_disabled`.
    The refusal is where the mode is set, rather than at the first permission
    request an hour into an agent's work.
14. `GET /v1/doctor` carries the same `PythonDto` the status does. The
    interpreter is reported apart from the tools (012, rule 19) because the
    question it answers is not whether it is there but whether it is new
    enough.
15. `python_bin` is a key of
   `<home>/config.toml`, listed by `ariadned --help` and read by
    `--check-config`. The test seam `ai_permissions_installer` — a command
    the daemon runs in place of the venv, package and weights, with the
    model home, run and Kev commit in its
   environment, its exit status deciding the install and its stderr becoming
    `last_error` — `ai_permissions_serve_command` and
    `ai_permissions_endpoint` are settings of the daemon alone. None is a key
    of the user's config, and a file naming one is refused like any other
    unknown key.
16. Where the model's server answers is `ai_permissions_endpoint` where it is
    configured, else whatever the server last reported, else nothing. The
    model is live — what a decision needs — only while it is enabled and
    something is serving it.
17. The routes of the model's old name are gone: nothing answers at them.
    Every error message, log line and the OpenAPI tag name the model "the AI
    permission model", never the package.

## Server

18. While the model is enabled and its install is ready, the daemon runs
    `<home>/ai-permissions/venv/bin/python -m kev.serve` as its child, in a
    process group of its own, with `--run` the pinned Hugging Face Hub run
    `decide::RUN` names (`bench/ai-permissions/winner.json`'s `run`), `--host`
    and `--port` an available loopback port. `HF_HOME` is
    `<home>/ai-permissions/hf` and `HF_HUB_OFFLINE=1`, so nothing downloads at
    startup: the install leaves everything the run needs on disk first. The
    server never listens beyond the local machine. It runs under a `/bin/sh`
    guard whose stdin is a pipe only the daemon writes to. When that pipe
    closes, however the daemon ended (`kill -9` included), the guard kills the
    whole process group, so no server outlives its daemon. When the server
    exits on its own, the guard exits with its status.
19. The daemon waits up to `Timeouts::ai_permissions_serve_start` for
    `GET /v1/models` to answer with a success. It then publishes the loopback
    endpoint in `ai_permissions_updated`. On disable, refresh, shutdown, or an
    unsuccessful health wait it clears that endpoint and kills the whole
    process group. A refresh restarts the server once: the daemon compares
    the row's `last_refresh_at` with the one its server started on, and
    starts a new server when they differ.
20. An unexpectedly exited server clears its endpoint and restarts with a
    capped backoff that starts at `Timeouts::ai_permissions_serve_restart`,
    1, 2, 4 … 60 seconds. A disabled model is not restarted.

## Decision configuration

21. Each decision asks one `noul` question: “Does this coding-agent tool call
    need a person's review?” Its `false` criterion is “git status, diff, log,
    show, add or commit; cargo, npm, make, tsc, pytest, eslint or prettier
    builds, tests and lints; ls, find, cat or grep; reading or editing files
    under the repository path; fetching documentation”. Its `true` criterion
    is “anything not listed as safe”. `false` means allow.
22. The status and update request do not carry the checkpoint or prompt texts.
23. The server passes `--run`, `decide::RUN`'s value, to `kev.serve`; the
    installer downloads what that run needs onto disk before the model is
    ready. The installer receives `AI_PERMISSIONS_HOME`, `AI_PERMISSIONS_RUN`,
    and `AI_PERMISSIONS_KEV_COMMIT`.
24. The default threshold is 0.56, `winner.json`'s. Existing settings rows
    keep their stored threshold.

## Decisions

25. In `ai`, the model decides before a learned approval is read. A request
    made while a launched server is still loading its weights waits until
    that server is healthy or given up on (at most
    `Timeouts::ai_permissions_serve_start`), so the requests of a freshly
    started daemon are still the model's. The daemon then posts to
    `{endpoint}/v1/systemone` and waits at most
    `Timeouts::ai_permissions_decision`, five seconds by default.
26. The state is a JSON object, not a string. It carries `tool`
    (`toolCall.title`), `kind`, `input` (the compact JSON of `rawInput`, cut
    at 2,000 characters) and `options` (option names joined by `, `), each
    left out where empty, then always these ten signals computed from the
    tool call and the repository alone, never the outcome: whether it
    operates inside the repository, whether it writes files, whether it
    writes outside the repository, whether it uses the network, the hosts it
    names, whether it reads a sensitive path, whether it is destructive,
    whether it escalates privilege, whether it changes a git remote, and
    whether it could exfiltrate data.
27. The request carries `model = kev-latest`, a label Kev accepts and echoes
    without using: the checkpoint actually served is fixed by `decide::RUN` at
    launch (rule 18). The one question is named `decision`, has type `noul`,
    and carries the built-in instruction and criteria.
28. The answer is `answers.decision.noul`, the probability that the request
    needs review; Kev's answer carries nothing else the daemon reads. The
    allow score is `1 - noul`. The model allows only when `false` is the
    argmax (`noul < 0.5`), the allow score meets the configured threshold,
    and the request has an allowing option.
29. Every other answer follows `learn` (021, rule 9): a matching learned
    approval is selected, otherwise the console is asked, and its allowing
    answer is remembered. The model is unavailable, and a warning is logged,
    when `live()` is absent, its call fails or times out, or its answer is
    malformed.
30. An allow of the model is never learned. Only an allowing console answer
    writes the learned table.
31. `permission.replied` carries `decided_by`: `ai`, `learned`, `console`,
    or `auto`, and always the keys `label`, `confidence`, `threshold`,
    `guardrail` and `ai_error`. Whenever the model answered, whoever decided,
    `label` is `allow` or `escalate`, `confidence` its allow score, and
    `threshold` the one that score was held to. `guardrail` names the rule
    that asked the console. `ai_error` is `unavailable`, `failed`,
    `timed out` or `malformed` where the model was asked and gave no answer.
    Each is null otherwise, and all five are null outside `ai`. The
    `permission_request` carries the same fields, those that are not null,
    since the model has answered before the console is asked. In `ai`, each
    reply also logs one `AI permission decision` line at INFO with the tool,
    `decided_by` and those fields. While a question waits, the console
    shows under its call why the model left it to a person —
    `AI said escalate (0.41, threshold 0.70)`,
    `guardrail credential-paths`, `AI timed out`
    (`ariadne_api::permissions::ai_permission_note`) — and
    `ariadne session logs` prints it under the question. The answered line
    is the option chosen, or `allowed by AI (0.94)` for a reply of the
    model. The reply's event summary (012, rule 13) names the reason too.

## Guardrails

32. The daemon embeds `bench/ai-permissions/guardrails.json` and compiles its
    regular expressions once during startup. An invalid expression stops
    startup and names its rule.
33. In `ai`, the daemon checks the rules in file order before it calls the
    model or reads a learned approval. A matching rule logs a warning, asks
    the console, and writes its name as `guardrail` on the
    `permission_request` payload. A request with no match has no `guardrail`
    field.
34. A rule can select `command`, `title`, `input`, or `path`. Its optional
    `names` and `kinds` lists restrict which tool calls it checks. `input` is
    the JSON text of `rawInput`; `path` uses the path sources and order from
    rule 26.

## Benchmark

35. `bench/ai-permissions/` holds the harness, cases, configurations, results,
    report, winner, guardrails, and parity fixture. The commands to reproduce
    the run are in `bench/ai-permissions/README.md` and `REPORT.md`.
36. The 2026-09-26 run at git sha
    `3ff094bd24006c8137c5008fd932fadb649970e3` tested 75 configurations. The
    winner covered 30 of 127 safe cases (23.62%) and 3 of 301 real requests
    (1.00%). It allowed none of 89 adversarial development cases, 168 held-out
    cases, or 44 elevated cases. Its AUROC was 0.9808, ECE was 0.4484, median
    single-request latency was 23.3 ms, and held-out margin was 0.0094.
37. Any change to the checkpoint, representation, question, threshold, or
    guardrails reruns the benchmark and regenerates
    `fixtures/winner-states.jsonl`.
38. The benchmark has thin margins, 1% real coverage, an `mps`-only run, and a
    moving local real-request set. Its regex guardrails inspect one request
    without understanding later execution. The elevated labels, long-command
    window, and device precision remain known limits.

## Acceptance criteria

- A fresh daemon is off, at threshold 0.56, with no schedule, and reports
  the interpreter it probed
  (`ai_permissions.rs::the_settings_start_at_the_defaults_with_the_interpreter_probed`).
- A version is read out of what an interpreter prints, and only 3.12 and 3.13 pass
  (`ai_permissions/python.rs::tests::the_version_is_read_from_the_line_and_checked_against_kevs_versions`);
  an interpreter that is not there is reported as missing
  (`::an_interpreter_that_is_not_there_is_reported_as_missing`).
- Turning the model on against Python 3.14 is refused with
  `python_unavailable` and changes nothing
  (`ai_permissions.rs::turning_the_model_on_without_a_new_enough_python_is_refused_and_changes_nothing`).
- Turning it on against 3.13 answers `installing`, says so on the stream, and
  settles on `ready` with the pin, the weights and the refresh time;
  the installer saw `<home>/ai-permissions`, the Kev run and the Kev commit
 (`ai_permissions.rs::turning_the_model_on_starts_the_install_and_reports_it_ready`).
- A venv with Python 3.13 is kept, and a venv with Python 3.14 is rebuilt
  (`ai_permissions/install.rs::tests::a_venv_with_a_supported_interpreter_is_kept`).
- An install that fails leaves the model on and carries the installer's own
  words
  (`ai_permissions.rs::an_install_that_fails_keeps_the_model_on_and_says_why`).
- `threshold` and `schedule` are kept and read back by the
  next daemon; 1.5 and `25:00` are refused with 422; `null` turns the
  schedule off and an absent one keeps it
  (`ai_permissions.rs::the_settings_are_validated_and_survive_a_daemon_restart`),
  and a schedule is two digits, a colon and two digits
  (`http/permissions.rs::tests::a_schedule_is_two_digits_a_colon_and_two_digits`).
- Refresh is refused with `ai_disabled` while off and `ai_busy` while
  installing, and runs the installer again on the fixed Kev pins
  (`ai_permissions.rs::refresh_is_refused_while_the_model_is_off_or_busy_and_reruns_the_install`).
- Turning the model off keeps the release and the weights
  (`ai_permissions.rs::turning_the_model_off_keeps_the_files_it_installed`).
- Turning the model off while an install runs keeps it off when that install
  ends, with the release written down
  (`ai_permissions.rs::turning_the_model_off_during_an_install_keeps_it_off_when_the_install_ends`);
  turning it on again meanwhile answers `installing` and settles on the
  install's end
  (`::turning_the_model_back_on_during_its_install_reports_that_install`); a
  turn-off that lands before that `installing` keeps the model off through
  the install's end
  (`::a_turn_off_that_lands_before_the_rejoin_keeps_the_model_off`); and a
  state written only while enabled leaves a row turned off at `disabled`
  (`store.rs::the_ai_permission_settings_are_one_row_that_takes_partial_writes`).
- The daily refresh starts once per local date, catches up after daemon
  startup, and skips a disabled or busy model
  (`ai_permissions.rs::the_daily_refresh_runs_the_install_once_at_its_minute`,
  `ai_permissions_server.rs::the_schedule_refreshes_once_per_local_day`).
- A repository is refused `ai` while the model is off, on registration and on
  an edit, and takes it once the model is on
  (`ai_permissions.rs::a_repository_takes_the_ai_mode_only_once_the_model_is_on`),
  and the store round-trips the mode
  (`store.rs::a_repository_takes_the_ai_permission_mode`).
- The configured endpoint wins over the one a server reports, and nothing is
  live while the model is off
  (`ai_permissions.rs::the_endpoint_is_the_configured_one_and_live_needs_the_model_on`).
- A ready enabled model starts its local server with its built-in run and
  reports the endpoint
  (`ai_permissions_server.rs::a_ready_model_starts_the_server_with_its_built_in_weights`),
  restarts it after a refresh and an unexpected exit
  (`::a_refresh_and_an_unexpected_exit_restart_the_server`), starts it again
  after a daemon restart and leaves no child after shutdown
  (`::a_ready_enabled_model_starts_after_a_daemon_restart`). A server that
  misses its health deadline has no endpoint
  (`::a_server_that_never_answers_health_is_not_live`).
- The guard kills the server once the daemon's end of its pipe closes, and
  exits with the server's status when the server exits on its own
  (`ai_permissions/server.rs::tests::the_server_dies_when_the_daemon_end_of_its_pipe_closes`,
  `::the_guard_exits_with_the_server_status`).
- The three paths, the schemas, the nullable schedule, the
  doctor's `python` and the event kind are in the OpenAPI document
  (`ai_permissions.rs::the_endpoints_the_schemas_and_the_event_are_in_the_openapi_document`),
  and the doctor reports the interpreter apart from the tools
  (`::the_doctor_reports_the_interpreter_the_model_needs`).
- The old route answers 404 (`ai_permissions.rs::the_old_route_answers_404`).
- The settings are one row taking partial writes and
  they survive a store reopen
  (`store.rs::the_ai_permission_settings_are_one_row_that_takes_partial_writes`).
- `python_bin` is read from `config.toml`, and `ai_permissions_release_url` is refused,
  and the test seams are not keys of it
  (`config.rs::tests::the_ai_permissions_keys_a_user_may_set_are_read_and_the_test_seams_are_not`).
- Every benchmark case builds the same model, questions, state, and guardrail
  as the committed fixture
  (`ai_permissions::decide::tests::every_benchmark_case_builds_the_winning_request_and_guardrail`).
- An invalid guardrail stops startup and names its rule
  (`ai_permissions::decide::tests::an_invalid_guardrail_stops_startup_and_names_the_rule`).
- A confident allow selects the allowing option, records `decided_by: "ai"`
  and its confidence, raises no attention, and sends the benchmarked state and
  `noul` question with `model = "kev-latest"` to the model
  (`ai_permissions_decisions.rs::a_confident_allow_runs_at_once_and_reports_ai`).
- Every reply keeps the model's side: the label, score and threshold that
  fell short, the guardrail that asked, or why the model gave no answer
  (`ai_permissions_decisions.rs::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval`,
  `::an_answer_that_needs_review_waits_for_the_console`,
  `::a_guardrail_asks_the_console_without_calling_the_model_and_names_the_rule`,
  `::a_malformed_answer_warns_and_waits_for_the_console`,
  `::a_stopped_model_warns_and_waits_for_the_console`,
  `::a_model_timeout_waits_for_the_console`,
  `::a_disabled_model_waits_for_the_console`), and each reply is logged
  (`::an_answer_that_needs_review_waits_for_the_console`).
- The request carries the model's answer while the question waits
  (`ai_permissions_decisions.rs::an_answer_that_needs_review_waits_for_the_console`,
  `::a_stopped_model_warns_and_waits_for_the_console`), and the console and
  `session logs` show it with the question, not with the answer
  (`tui/picker.rs::tests::a_waiting_question_says_why_the_model_left_it_and_its_answer_does_not`,
  `transcript.rs::tests::a_question_says_why_the_ai_permission_model_left_it_to_the_console`,
  `ariadne_api::permissions::tests::a_reply_names_why_the_model_did_not_decide_it`).
- A `noul` answer is gated on the probability of its false, allowing side
  (`ai_permissions_decisions.rs::a_noul_answer_is_gated_on_its_allow_probability`).
- An uncertain allow asks the console, remembers its approval, and still asks
  the model before selecting that learned approval next time
  (`ai_permissions_decisions.rs::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval`).
- An answer whose argmax needs review asks the console
  (`ai_permissions_decisions.rs::an_answer_that_needs_review_waits_for_the_console`).
- A guardrail asks the console, does not call the model, and names its rule on
  the request. A learned approval does not answer it
  (`ai_permissions_decisions.rs::a_guardrail_asks_the_console_without_calling_the_model_and_names_the_rule`,
  `::a_learned_approval_does_not_answer_a_guardrail_request`).
- A confident allow without an allowing option asks the console
  (`ai_permissions_decisions.rs::an_allow_without_an_allowing_option_waits_for_the_console`).
- A request made while the server loads waits for it, and the model decides
  it
  (`ai_permissions_decisions.rs::a_request_made_while_the_server_loads_waits_for_it`).
- A stopped or timed-out model warns and asks the console
  (`ai_permissions_decisions.rs::a_stopped_model_warns_and_waits_for_the_console`,
  `::a_model_timeout_waits_for_the_console`).
- A malformed answer warns and asks the console
  (`ai_permissions_decisions.rs::a_malformed_answer_warns_and_waits_for_the_console`).
- A model disabled after the repository chose `ai` asks the console
  (`ai_permissions_decisions.rs::a_disabled_model_waits_for_the_console`).
- The unchanged `auto`, `ask`, and `learn` paths name their decider
  (`acp_runtime.rs::auto_approves_a_permission_request_with_the_allowing_option`,
  `acp_console.rs::ask_raises_attention_and_a_console_answer_unblocks_the_turn`,
  `::learn_remembers_an_approval_per_repository_across_a_daemon_restart`).
- A reply of the model renders its decider and confidence
  (`ariadne-console::tui::picker::tests::an_ai_answer_names_the_model_and_its_confidence`).

## Sources

The AI permission model is the upstream package
[`kev`](https://github.com/jaredpalmer/kev) at a pinned commit, installed
from its `serve` extra (not the unrelated PyPI package of the same name). The
daemon keeps to its interface: `<venv>/bin/python -m kev.serve --run <run>
--host <host> --port <port>`, `HF_HOME` and `HF_HUB_OFFLINE=1` so nothing
downloads once the model is serving, and `GET /v1/models` for health. `<run>`
is `decide::RUN`, the Hugging Face Hub id `bench/ai-permissions/winner.json`
pins (`jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`), which
the installer downloads onto disk before the server is ready.

`crates/ariadne-core/src/lib.rs`,
`crates/ariadne-api/src/permissions.rs`,
`crates/ariadne-api/src/stream.rs`,
`crates/ariadne-api/src/doctor.rs`,
`crates/ariadne-client/src/lib.rs`,
`crates/ariadne-client/src/endpoint.rs`,
`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/src/ai_permissions.rs`,
`crates/ariadne-daemon/src/ai_permissions/`,
`crates/ariadne-daemon/src/main.rs`,
`crates/ariadne-daemon/src/http/permissions.rs`,
`crates/ariadne-daemon/src/http/repositories.rs`,
`crates/ariadne-daemon/src/http/doctor.rs`,
`crates/ariadne-daemon/src/config.rs`,
`crates/ariadne-daemon/src/acp.rs`.
