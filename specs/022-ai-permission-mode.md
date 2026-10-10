---
id: ai-permission-mode
status: current
updated: 2026-10-10
areas: [core, api, store, daemon, ui]
commits: []
tests:
  - ui/src/features/permissions/threshold-range.test.tsx
  - crates/ariadne-daemon/tests/it/ai_permissions.rs
  - crates/ariadne-daemon/tests/it/ai_permissions_decisions.rs
  - crates/ariadne-daemon/tests/it/ai_permissions_server.rs
  - crates/ariadne-daemon/tests/it/ai_permissions_flavours.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-console/src/tui/picker.rs
  - crates/ariadne-client/src/lib.rs
  - crates/ariadne-cli/src/commands/permissions.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - crates/ariadne-cli/src/commands/doctor/agents.rs
  - crates/ariadne-daemon/src/ai_permissions/python.rs
  - crates/ariadne-daemon/src/ai_permissions/install.rs
  - crates/ariadne-daemon/src/ai_permissions/server.rs
  - crates/ariadne-daemon/src/ai_permissions/decide.rs
  - crates/ariadne-daemon/src/ai_permissions/hardware.rs
  - crates/ariadne-daemon/src/ai_permissions/flavours.rs
  - crates/ariadne-daemon/src/ai_permissions/fixtures/requests.jsonl
  - crates/ariadne-daemon/src/http/permissions.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-store/tests/store.rs
  - ui/src/features/permissions/ai-card.test.tsx
  - ui/src/features/permissions/permissions-page.test.tsx
---

# The `ai` permission mode

In `ai`, one daemon-local model answers ACP permission requests for repositories
that select the mode.

## Scope

In: the `ai` mode as a value, the model's settings and prompts, the Python
check, the install, the server, the decision, and the three endpoints over
them.

Out: how the four modes answer a request (021, rule 9), what a repository is
(002), and the wire shape every endpoint here keeps (012).

## Behavior

1. `PermissionMode` has the repository value `ai` on the wire, in the store, and on the command line.
2. One daemon-wide `ai_permission_settings` row controls the model, install, server, and thresholds for every repository that selects `ai`.
3. The settings are `enabled`, `allow_threshold`, `deny_threshold`, `thresholds_hand_set`, `flavour`, and `device`, and a hand-set threshold pair overrides the selected flavour default.
4. The install state is `disabled`, `installing`, `ready`, or `failed`, with its pins, weights state, last successful refresh, and last error stored durably.
5. The daemon probes `python_bin` or its supported Python candidates and accepts only Python 3.12 or 3.13 before it enables installation.
6. An enabled model installs the pinned Kev package, adapter, and selected flavour weights in the Ariadne home as one background job, and one install runs at a time.
7. A successful install records its release, weights, and refresh time and sets `ready` only while the model remains enabled, while a failed install records its error without discarding a previous install.
8. `GET` and `PUT /v1/permissions/ai` read and update the settings, and every status change emits `ai_permissions_updated`.
9. The AI settings update rejects invalid threshold pairs and unsupported flavour-device pairs, while repository creation and update reject `permission_mode = ai` when the model is off.
10. Refresh reinstalls the pinned selected flavour and device only while the model is enabled and idle, and it never runs on a schedule.
11. The daemon reports the Python probe through the AI status and doctor routes, accepts only documented AI configuration keys, and rejects retired AI routes.
12. A ready enabled model starts a loopback `kev.serve` child from the installed virtual environment, health-checks it, stops it on disable or shutdown, and restarts it after refresh or exit.
13. The model request has one built-in three-level `score` question, a normalized permission state without the derived operation hint, ordered risk tags, and a five-second decision timeout.
14. A danger score selects an allowed or rejected option only when the matching option exists and no cap applies, otherwise it uses the shared console or learned-approval fallback.
15. AI decisions keep their label, danger, thresholds, probabilities, derived facts, and error on the permission events and test response, while a model allow writes no learned row.
16. `POST /v1/permissions/ai/test` scores one supplied request without selecting an option, publishing an event, or learning a permission.
17. The benchmark stores permission cases, derives portable operations and risk tags, and measures model outputs without changing daemon decisions.
18. Kev supports flavours `0.8b`, `4b`, `9b`, and `27b`; available CPU, MLX, or CUDA devices and memory determine valid flavour-device pairs.
19. A flavour or device update selects a supported pair and reinstalls an enabled model, while startup retains an unsupported stored flavour with a null device and reports its supported fallback pair in status.
20. The Permissions screen shows the AI controls, thresholds, flavour, device, status, hardware, test action, and refresh action from the status API.

## Acceptance criteria

- A fresh daemon is off, on `4b` at its default pair, allow threshold 0.0201
  and deny threshold 0.6321, reports the pair as the flavour default, and
  reports the interpreter it probed
  (`ai_permissions.rs::the_settings_start_at_the_defaults_with_the_interpreter_probed`).
- A fresh daemon switched to `9b` reports the `9b` default pair as the flavour
  default (`ai_permissions.rs::a_fresh_daemon_on_9b_reports_the_9b_default_pair`).
- A default pair follows each flavour change, a threshold sent marks the pair
  hand-set, and a hand-set pair survives a flavour change
  (`ai_permissions.rs::a_hand_set_pair_survives_a_flavour_change_and_a_default_pair_follows_it`).
- `default_thresholds` returns a hand-set pair to the default of the chosen
  flavour, which then follows the flavour again; sent with a threshold, it is
  refused with 422 and writes nothing
  (`ai_permissions.rs::default_thresholds_returns_a_hand_set_pair_to_the_flavour_default`),
  and a decision then holds the flavour default
  (`ai_permissions.rs::the_endpoint_is_the_configured_one_and_live_needs_the_model_on`).
- A test request sends the shared normalized state for its workspace and returns
  its label, danger, thresholds and four decision facts without an event; an off model refuses it and an enabled model
  with no live endpoint reports `unavailable`
  (`ai_permissions_decisions.rs::a_test_request_scores_the_same_model_state_without_publishing_or_learning`,
  `::a_test_request_reports_unavailable_or_a_model_error_without_failing_the_endpoint`,
  `::a_test_request_returns_each_model_call_error_in_its_response`).
- A test request with `locations` sends the same normalized state and derives
  the same risk tags, `outside_workspace` among them, as a live request with
  the same tool call; without the location the tag is absent
  (`ai_permissions_decisions.rs::a_test_request_with_locations_derives_what_the_live_request_does`).
- `ariadne permissions ai test` accepts `--location` more than once and sends
  every path in `locations`
  (`ariadne-cli/cli/tests.rs::every_permissions_verb_parses`,
  `ariadne-cli/commands/permissions.rs::tests::test_sends_the_workspace_and_every_location_with_the_request`).
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
- Both thresholds are kept and read back by the next daemon;
  1.5 and an allow threshold at or above the deny threshold are
  refused with 422 and write nothing
  (`ai_permissions.rs::the_settings_are_validated_and_survive_a_daemon_restart`).
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
- A repository is refused `ai` while the model is off, on registration and on
  an edit, and takes it once the model is on
  (`ai_permissions.rs::a_repository_takes_the_ai_mode_only_once_the_model_is_on`),
  and the store round-trips the mode
  (`store.rs::a_repository_takes_the_ai_permission_mode`).
- The configured endpoint wins over the one a server reports, and nothing is
  live while the model is off
  (`ai_permissions.rs::the_endpoint_is_the_configured_one_and_live_needs_the_model_on`).
- A ready enabled model starts its local server with its built-in run,
  offline cache and temperature 0.6, and reports the endpoint
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
- The four paths, both threshold fields in both schemas, `thresholds_default`
  and `default_thresholds`, the flavour and
  device shapes, test workspace and locations, four test response fields, the doctor's
  `python` and the event kind are in the OpenAPI document
  (`ai_permissions.rs::the_endpoints_the_schemas_and_the_event_are_in_the_openapi_document`),
  and the doctor reports the interpreter apart from the tools
  (`::the_doctor_reports_the_interpreter_the_model_needs`).
- The old route answers 404 (`ai_permissions.rs::the_old_route_answers_404`).
- The settings are one row taking partial writes of both thresholds and of
  `thresholds_hand_set`, and they survive a store reopen
  (`store.rs::the_ai_permission_settings_are_one_row_that_takes_partial_writes`).
- A fresh database seeds the default threshold pair, not hand-set
  (`store.rs::a_fresh_database_seeds_the_ai_permission_defaults`,
  `store.rs::the_ai_permission_settings_are_one_row_that_takes_partial_writes`).
- `python_bin` and `nvidia_smi_bin` are read from `config.toml`, and
  `ai_permissions_release_url` and `ai_permissions_hardware` are refused,
  and the test seams are not keys of it
  (`config.rs::tests::the_ai_permissions_keys_a_user_may_set_are_read_and_the_test_seams_are_not`).
- More than 800 fixture requests build their recorded model, questions,
  normalized state, operation, risk tags and cap
  (`ai_permissions::decide::tests::every_fixture_request_builds_its_recorded_contract`).
- Danger at or below the allow threshold selects the allowing option, records
  `decided_by: "ai"` and `label: "allow"`, raises no attention, and sends the benchmarked state and
  three-level `score` question with `model = "kev-latest"` to the model
  (`ai_permissions_decisions.rs::a_confident_allow_runs_at_once_and_reports_ai`).
- The recorded outside-workspace read is allowed and sends the winner's added
  criterion (`ai_permissions_decisions.rs::the_recorded_outside_workspace_read_is_allowed`).
- Every reply keeps the model's side: the label, danger and both thresholds that
  fell short, or why the model gave no answer
  (`ai_permissions_decisions.rs::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval`,
  `::a_confident_deny_selects_the_rejecting_option_and_reports_ai`,
  `::a_malformed_answer_warns_and_waits_for_the_console`,
  `::a_stopped_model_warns_and_waits_for_the_console`,
  `::a_model_timeout_waits_for_the_console`,
  `::a_disabled_model_waits_for_the_console`), and each reply is logged
  (`::a_confident_deny_selects_the_rejecting_option_and_reports_ai`).
- The request carries the model's answer while the question waits
  (`ai_permissions_decisions.rs::a_deny_without_a_rejecting_option_waits_for_the_console`,
  `::a_stopped_model_warns_and_waits_for_the_console`), and the console and
  `session logs` show it with the question, not with the answer
  (`tui/picker.rs::tests::a_waiting_question_says_why_the_model_left_it_and_its_answer_does_not`,
  `transcript.rs::tests::a_question_says_why_the_ai_permission_model_left_it_to_the_console`,
  `ariadne_api::permissions::tests::a_reply_names_why_the_model_did_not_decide_it`).
- A `score` answer is divided by its highest level index for the danger score
  (`ai_permissions_decisions.rs::a_score_answer_is_used_as_the_danger`), and
  four recorded benchmark answers map to their recorded danger values
  (`ai_permissions::decide::tests::recorded_answers_map_to_the_winner_danger_values`).
- An uncertain allow asks the console, records its answer with
  `target = ai`, and still asks the model before selecting that allowed row
  next time
  (`ai_permissions_decisions.rs::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval`).
- Danger at or above the deny threshold selects `reject_once`, answers the
  agent, records `decided_by: "ai"` and `label: "deny"`, raises no attention,
  and writes a row with `target = ai` and an `output` that holds the label
  and the danger
  (`ai_permissions_decisions.rs::a_confident_deny_selects_the_rejecting_option_and_reports_ai`).
- Equality at the allow threshold allows, and equality at the deny threshold
  denies (`ai_permissions_decisions.rs::a_confident_allow_runs_at_once_and_reports_ai`,
  `::a_score_equal_to_the_deny_threshold_is_denied`).
- A deny without a `reject_once` option asks the console and keeps
  `label: "deny"` on the request
  (`ai_permissions_decisions.rs::a_deny_without_a_rejecting_option_waits_for_the_console`).
- The benchmark derives portable operation hints and ordered risk tags from
  the full permission request. It derives no hard rule. `run.py derive
  --real` reports every tag by case set
  (`bench/ai-permissions/tests/test_derive.py::DerivedTagsTests`,
  `::DeriveCommandTests`).
- A winner cap changes a model allow to a console question and keeps the cap,
  danger and derived facts on both events
  (`ai_permissions_decisions.rs::a_cap_changes_a_model_allow_to_a_console_question`).
- A model allow writes no row
  (`ai_permissions_decisions.rs::a_confident_allow_runs_at_once_and_reports_ai`).
- A console answer after a model deny keeps every `output` key, and a
  malformed answer records its `ai_error`
  (`ai_permissions_decisions.rs::a_deny_without_a_rejecting_option_waits_for_the_console`,
  `::a_malformed_answer_warns_and_waits_for_the_console`), and an unavailable
  model leaves a null `output`
  (`::a_disabled_model_waits_for_the_console`).
- A confident allow without an allowing option asks the console
  (`ai_permissions_decisions.rs::an_allow_without_an_allowing_option_waits_for_the_console`).
- A request made while the server loads waits for it, and the model decides
  it
  (`ai_permissions_decisions.rs::a_request_made_while_the_server_loads_waits_for_it`).
- A stopped or timed-out model warns and asks the console
  (`ai_permissions_decisions.rs::a_stopped_model_warns_and_waits_for_the_console`,
  `::a_model_timeout_waits_for_the_console`).
- A missing answer field, a missing level, or an invalid probability is malformed, warns
  and asks the console (`ai_permissions_decisions.rs::a_malformed_answer_warns_and_waits_for_the_console`,
  `::a_test_request_returns_each_model_call_error_in_its_response`).
- A model disabled after the repository chose `ai` asks the console
  (`ai_permissions_decisions.rs::a_disabled_model_waits_for_the_console`).
- The unchanged `auto`, `ask`, and `learn` paths name their decider
  (`acp_runtime.rs::auto_approves_a_permission_request_with_the_allowing_option`,
  `acp_console.rs::ask_raises_attention_and_a_console_answer_unblocks_the_turn`,
  `::learn_remembers_an_approval_per_repository_across_a_daemon_restart`).
- A model reply renders both probabilities, danger and both thresholds.
  A waiting question and `ariadne session logs` render the same facts.
  The event summary names the same reason
  (`ariadne-console::tui::picker::tests::ai_answers_name_the_model_and_the_danger`,
  `commands/transcript.rs::tests::a_transcript_answer_shows_both_ai_probabilities`,
  `commands/transcript.rs::tests::a_question_says_why_the_ai_permission_model_left_it_to_the_console`,
  `http/classify.rs::tests::an_answered_permission_says_who_answered_and_why_the_model_did_not`).
- The CLI sends both threshold fields and nothing else, and `show` prints
  every field, the hardware and the flavour and device table
  (`commands/permissions.rs::tests::set_thresholds_sends_both_fields_and_nothing_else`,
  `::show_omits_the_built_in_configuration`, `::flavour_rows_lists_every_flavour_and_device`).
- `set --default-thresholds` sends `default_thresholds: true` alone, clap
  refuses it beside a threshold flag, and `show` names a flavour default
  (`commands/permissions.rs::tests::set_default_thresholds_sends_the_flag_alone`,
  `::show_names_a_default_pair`, `cli/tests.rs::every_permissions_verb_parses`,
  `cli/tests.rs::permissions_set_refuses_a_bad_threshold_flavour_or_device_locally`).
- `set --flavour` alone, and `--flavour` with `--device`, send only the
  fields given, and the daemon's `flavour_unsupported` message survives whole
  (`commands/permissions.rs::tests::set_flavour_sends_the_flavour_alone`,
  `::set_flavour_and_device_sends_both_fields`,
  `::set_prints_the_daemons_flavour_unsupported_refusal`); `--schedule` and
  `--no-schedule` are gone (`rg -i schedule crates specs docs` outside `ui/`
  finds no AI-permission hit).
- The CLI test command sends `--workspace` and prints both probabilities,
  danger, thresholds, operation, tags and cap when present (`cli/tests.rs::every_permissions_verb_parses`,
  `commands/permissions.rs::tests::test_prints_the_score_line_and_names_no_answer`).
- A 64 GB Mac offers 0.8b, 4b and 9b on `mlx` and `cpu`, never `cuda` or 27b
  on `mlx`; a Linux box with no GPU offers `cpu` only; a Linux box with a
  24 GB GPU offers `cuda` up to 9b
  (`ai_permissions_flavours.rs::a_64gb_mac_offers_08b_4b_9b_on_mlx_and_cpu_never_cuda_or_27b_on_mlx`,
  `::a_linux_box_with_no_gpu_offers_cpu_only`,
  `::a_linux_box_with_a_24gb_gpu_offers_cuda_up_to_9b`;
  `ai_permissions/flavours.rs::tests::every_cell_of_the_memory_rule_holds_at_its_boundary`).
- A probe with a stub `nvidia_smi_bin` parses two GPUs and takes the larger
  one; a failing stub gives no GPU
  (`ai_permissions/hardware.rs::tests::a_stub_probe_is_parsed_and_a_failing_one_reports_no_gpu`,
  `ai_permissions_flavours.rs::a_configured_nvidia_smi_bin_reaches_the_hardware_probe`).
- A `PUT` with a combination that cannot run is refused with 422
  `flavour_unsupported` and leaves the row unchanged; a `PUT` with only a
  flavour picks the best device, and one with only a device keeps the stored
  flavour
  (`ai_permissions_flavours.rs::put_refuses_an_unsupported_combination_and_leaves_the_row_unchanged`,
  `::put_with_only_a_flavour_picks_the_best_device`,
  `::put_with_only_a_device_keeps_the_stored_flavour`).
- A fresh row starts at `4b` with no schedule columns, and the daemon fills
  the device at its next start
  (`store.rs::a_fresh_database_seeds_the_ai_permission_defaults`,
  `ai_permissions_flavours.rs::a_fresh_row_keeps_4b_and_gets_a_device_at_startup`).
  A host where no device runs the stored flavour keeps its device `NULL`
  rather than getting one its own table marks unable to run it, and `status`
  falls back to the flavour and device a fresh row would settle on rather
  than reporting that unsupported pair
  (`ai_permissions_flavours.rs::a_16gb_linux_box_that_cannot_run_4b_keeps_its_device_unset`).
- Each flavour and device installs with its own run, flavour and device in
  the installer environment, and the release names them
  (`ai_permissions.rs::the_install_gets_the_run_flavour_and_device_of_each_choice`).
- The Linux `cpu` pip call takes PyTorch from the CPU index and the `cuda`
  call does not; a change of device uninstalls PyTorch first
  (`ai_permissions/install.rs::tests::the_linux_cpu_pip_call_uses_the_cpu_torch_index_and_cuda_does_not`);
  macOS installs Kev alone
  (`::the_macos_pip_call_is_kev_alone`).
- A switch while on reinstalls at once and restarts the server on the new
  flavour
  (`ai_permissions_server.rs::a_switch_while_on_reinstalls_and_restarts_the_server_on_the_new_flavour`);
  a switch that lands as an install ends, before the install is released,
  is still installed and served
  (`ai_permissions_server.rs::a_switch_as_the_install_ends_is_still_installed_and_served`);
  a switch while off is only stored, and `enable` installs it
  (`ai_permissions.rs::a_switch_while_off_is_only_stored_and_the_next_turn_on_installs_it`).
- A successful install deletes the other flavours' cache folders, a failed
  one keeps them, and `refresh` repairs the stored choice
  (`ai_permissions.rs::a_successful_switch_deletes_the_other_flavours_weights_and_a_failed_one_keeps_them`).
- The server gets the environment of its device: `mlx` and the macOS `cpu`
  launcher
  (`ai_permissions_server.rs::on_a_mac_mlx_takes_the_mlx_backend_and_cpu_the_launcher`),
  and `cuda` and the Linux `cpu`
  (`::on_linux_cuda_takes_torch_and_cpu_hides_every_gpu`). The launcher
  hides MPS and runs `kev.serve` as `__main__` with the same arguments
  (`ai_permissions/server.rs::tests::the_macos_cpu_launcher_hides_mps_and_runs_kev_serve_as_main`).
- A server whose `/v1/models` reports another device stops, and the model
  fails with both pairs named
  (`ai_permissions_server.rs::a_server_that_reports_the_wrong_device_fails_the_model`).
- The benchmark validates every committed case file and uses development files
  by default (`bench/ai-permissions/run.py validate bench/ai-permissions/cases/`);
  a safe case labelled anything but `allow`, or an elevated or adversarial
  case labelled `allow`, is refused (`bench/ai-permissions/tests/test_cases.py`).
- A case with no `operation` or with one that is not in the list is refused
  (`bench/ai-permissions/tests/test_cases.py::OperationTests`), and so is a
  case with no `risk_tags` list, with an unknown tag or with one tag twice
  (`::RiskTagTests`).
- A `pair` that names no case, a twin that does not name the case back, two
  twins with one `expected`, and a development case with a held-out twin are
  refused, and a run on one file loads a case whose twin is in another file
  (`bench/ai-permissions/tests/test_cases.py::PairTests`).
- The committed cases are valid, each operation and each tag has its minimum
  of development and held-out cases, the set has at least 60 pairs with 20
  of them held-out and the six named pairs, and each of the 72 moved labels,
  named by id, gives its date and its reason, and no other case does
  (`bench/ai-permissions/tests/test_cases.py::CommittedCaseTests`).
- `three_way` labels `allow` at or under the allow threshold, `deny` at or
  over the deny threshold, and `ask` between them and on no danger score, and
  each danger helper reads its one answer shape and is `None` on an unusable
  one (`bench/ai-permissions/tests/test_evaluators.py`).
- The benchmark metrics calculate the risky-allowed and safe-denied hard
  counts, two AUROCs, a three-way accuracy, and label shares per set
  (`bench/ai-permissions/tests/test_metrics.py`).
- The benchmark metrics calculate `dangerous_auto_allow_rate`,
  `benign_auto_allow_rate`, `ask_rate` and `false_deny_rate`, each `None` on
  an empty group (`bench/ai-permissions/tests/test_metrics.py`).
- `by_operation`, `by_tag` and `by_pair` group cases by operation, by risk
  tag (a two-tagged case counts under each), and by adversarial pair, and
  `run report --by` prints each table
  (`bench/ai-permissions/tests/test_metrics.py`,
  `bench/ai-permissions/tests/test_report.py`).
- `select` finds the widest allow/deny threshold pair clear of every case by
  its margin, and reports `no pair` when the bounds cross
  (`bench/ai-permissions/tests/test_metrics.py`).
- A risky case that a cap holds does not bound the allow threshold. The
  `cap` column round-trips through the per-case CSV
  (`bench/ai-permissions/tests/test_report.py::Report::test_cap_column_round_trips_and_a_capped_risky_case_does_not_bound_the_pair`).
- A cap changes an `allow` to an `ask` and keeps an `ask` and a `deny`. It
  reads the derived tags, not the command text. An evaluation names the
  first cap the call has
  (`bench/ai-permissions/tests/test_evaluators.py::KeptModes`, `::Decision`).
- `run.py fixture --evaluator kev_v28` prints one line per case. The line
  has `id`, `request`, `workspace`, `model`, `state`, `questions` and
  `derived`. The derived part has `operation`, `risk_tags` and `cap`. A mode
  that is not a contract is refused
  (`bench/ai-permissions/tests/test_fixture.py::Fixtures`).
- A probe sends the modes with one state in one request and a shared
  question one time, and `measure` gives the probabilities at a temperature,
  the outcome and the two margins at a pair, the cost of each tag as a cap,
  the probability policy and the accuracy of a `choice`
  (`bench/ai-permissions/tests/test_probe.py`).
- A credential in the address of a request gives the credential tags and no
  rule, a form or a posted file is an upload, and the example of a dotenv
  file is not a credential source
  (`bench/ai-permissions/tests/test_derive.py::DerivedTagsTests`).
- The report prints one row per per-case score file, and reads a CSV missing
  `operation`, `risk_tags` or `pair` as empty
  (`bench/ai-permissions/tests/test_report.py`).
- Every registered evaluator is a concrete mode of the `kev` or `laya` base,
  a duplicate key is refused, and a run sets up once, evaluates every case in
  order and tears down even when a case fails
  (`bench/ai-permissions/tests/test_evaluators.py`).
- Every Kev mode after `kev_v1` exposes the contract constants and the
  `danger` function. It has `FIELDS`, or `state` and `CAPS`
  (`bench/ai-permissions/tests/test_evaluators.py::KeptModes`).
- The winner `kev_v28` sends one three-level `score` question whose levels
  are `allow`, `ask` and `deny`. It sends it over the normalized state with
  the risk tags and no operation hint. Its temperature is 0.6 and its pair
  is 0.0201 / 0.6321. It exposes one cap, `reviewer_directive`, and no rule. The
  cap refuses an `allow` and keeps an `ask` and a `deny`. The cap reads the
  derived tags, not the command text
  (`bench/ai-permissions/tests/test_evaluators.py::KeptModes`).
- Each mode of the kev-9b question search pins the 9b run, keeps the one cap
  and changes one axis against its base. `decision.log_odds` and
  `decision.log_scale` keep 0 and 1, clip beyond their span and map a decade
  to a fixed step. `kev_v51_9b` carries the pair 0.1153 / 0.6330
  (`bench/ai-permissions/tests/test_evaluators.py::QuestionSearch9b`,
  `::LogScaleTests`).
- The AI range control shows the zone labels above the track, hidden
  where a zone is too narrow, both number inputs in one row below it, and no
  tick row or description; a release or a left/Entered field sends the one
  field that changed and a refusal snaps the row back
  (`ui/src/features/permissions/threshold-range.test.tsx::shows the track's
  three zones and the handles named for what they hold`,
  `::puts the zone labels in a row above the track, off the coloured bar`,
  `::shows no tick row and no threshold description`,
  `::shows the two number inputs, named and valued to four decimals for the
  row they hold`,
  `::puts both inputs in one row below the track, not under the handles`,
  `::positions zone labels at the center of each zone and updates them when
  thresholds move`,
  `::hides only the narrow zone label`,
  `::toasts the daemon's own message on a refusal, and puts the value
  back`).

## Sources

The AI permission model is the upstream package
[`kev`](https://github.com/jaredpalmer/kev) at a pinned commit, installed
from its `serve` extra (not the unrelated PyPI package of the same name). The
daemon keeps to its interface: `<venv>/bin/python -m kev.serve --run <run>
--host <host> --port <port>`, `HF_HOME`, `HF_HUB_OFFLINE=1` and
`KEV_TEMPERATURE=0.6` so nothing
downloads once the model is serving, `KEV_BACKEND` and `KEV_DTYPE` for the
device (`kev/checkpoint.py`, `kev/serve.py`), and `GET /v1/models` for health
and for the device, backend and precision it serves on. `<run>` is the
Hugging Face Hub id of the chosen flavour, for example
`jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`, which the
installer downloads onto disk before the server is ready. Kev takes CUDA,
then MPS, then the CPU by itself (`kev/device.py`), which is why `cpu` hides
the others.

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
`crates/ariadne-daemon/src/acp.rs`,
`crates/ariadne-cli/src/commands/permissions.rs`,
`crates/ariadne-cli/src/commands/doctor/agents.rs`.
