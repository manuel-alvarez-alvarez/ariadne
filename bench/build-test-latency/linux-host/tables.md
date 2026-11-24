## Stages

| Stage | Command | Cold s, min / median / max | Warm s, min / median / max | Exits |
| --- | --- | ---: | ---: | --- |
| bins | `cargo build --locked --workspace --bins` | 103.077 / 104.391 / 104.499 (n=3) | 0.768 / 1.270 / 1.670 (n=3) | 0,0,0,0,0,0 |
| testbuild | `cargo nextest run --workspace --profile ci --no-run` | 147.232 / 148.100 / 148.728 (n=3) | 1.019 / 1.069 / 1.119 (n=3) | 0,0,0,0,0,0 |
| nextest | `cargo nextest run --workspace --profile ci --status-level fail --final-status-level fail` | 79.191 / 79.685 / 79.726 (n=3) | 79.334 / 79.660 / 80.696 (n=3) | 100,100,0,0,0,0 |
| nextest runner summary (cold) | from `Summary [..]` | 77.528 / 78.035 / 78.112 (n=3) | | |
| nextest runner summary (warm) | from `Summary [..]` | | 78.131 / 78.470 / 79.520 (n=3) | |
| doctest | `cargo test --doc --workspace` | 97.082 / 98.791 / 99.346 (n=3) | 3.777 / 3.826 / 4.579 (n=3) | 0,0,0,0,0,0 |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 90.055 / 90.501 / 93.884 (n=3) | 0.868 / 0.868 / 0.968 (n=3) | 0,0,0,0,0,0 |
| tauri | `cargo check` | 58.491 / 58.990 / 60.246 (n=3) | 0.467 / 0.668 / 0.718 (n=3) | 0,0,0,0,0,0 |
| desktop-ci | `npm ci` | 5.729 / 5.729 / 6.035 (n=3) | n/a | 0,0,0 |
| dashboard-ci | `npm ci` | 10.596 / 14.206 / 14.907 (n=3) | n/a | 0,0,0 |
| desktop-test | `npm test` | 113.531 / 113.729 / 113.732 (n=3) | 108.114 / 108.460 / 108.673 (n=3) | 0,0,0,0,0,0 |
| dashboard-test | `npm test` | 5.288 / 5.330 / 5.831 (n=3) | 5.280 / 5.329 / 5.881 (n=3) | 0,0,0,0,0,0 |
| build | `npm run build` | 29.758 / 30.061 / 30.819 (n=3) | 28.270 / 29.312 / 29.961 (n=3) | 0,0,0,0,0,0 |

## Rust tests

| Rank | Binary or file | Test | Min s | Median s | Max s |
| ---: | --- | --- | ---: | ---: | ---: |
| 1 | `ariadne-daemon::it` | `pull_request_babysit::asking_keeps_another_authors_request_until_the_merge` | 7.471 | 8.483 | 8.610 |
| 2 | `ariadne-daemon::it` | `pull_request_babysit::asking_keeps_a_request_of_mine_by_hand_until_the_merge` | 7.889 | 8.457 | 9.114 |
| 3 | `ariadne-daemon::it` | `pull_request_babysit::asking_keeps_a_dependabot_request_until_the_merge` | 7.962 | 8.380 | 8.992 |
| 4 | `ariadne-daemon::it` | `pull_request_reviews::a_review_round_needs_an_ask_and_runs_on_the_current_head` | 7.992 | 8.226 | 8.539 |
| 5 | `ariadne-daemon::it` | `pull_request_reviews::a_reply_in_a_review_thread_starts_one_round` | 6.082 | 6.637 | 7.440 |
| 6 | `ariadne-daemon::it` | `hosts::updates_wait_for_idle_and_retry_without_replacing_installed_binaries` | 6.051 | 6.473 | 6.745 |
| 7 | `ariadne-daemon::it` | `doctor::doctor_reports_a_missing_forced_place_and_its_reason` | 5.915 | 6.091 | 6.487 |
| 8 | `ariadne-daemon::it` | `kept_requests::an_ariadne_review_of_a_kept_request_reaches_its_pr_agent` | 5.783 | 5.992 | 6.262 |
| 9 | `ariadne-daemon::it` | `pull_request_babysit::keeper_push_and_external_push_reach_the_reviewer_independently` | 5.511 | 5.937 | 6.814 |
| 10 | `ariadne-daemon::it` | `place::a_herdr_server_that_does_not_answer_is_not_found` | 5.144 | 5.449 | 5.487 |
| 11 | `ariadne-daemon::it` | `pull_request_reviews::a_forge_re_request_round_waits_for_the_read_of_the_new_head` | 5.080 | 5.336 | 5.867 |
| 12 | `ariadne-daemon::it` | `hosts::daemon_version_installs_the_recorded_release_of_a_release_daemon` | 4.700 | 5.059 | 5.351 |
| 13 | `ariadne-daemon::it` | `pull_request_babysit::stopping_babysitting_keeps_a_dependabot_reviewer` | 4.506 | 4.872 | 5.067 |
| 14 | `ariadne-daemon::it` | `pull_request_reviews::a_push_reaches_an_idle_session_without_a_wait` | 4.618 | 4.822 | 5.216 |
| 15 | `ariadne-daemon::it` | `pull_request_babysit::a_keeper_whose_agent_dies_the_moment_it_starts_gives_up_after_the_budget` | 4.301 | 4.822 | 5.293 |
| 16 | `ariadne-daemon::it` | `host_reconnect::a_down_forward_starts_again_at_once_and_waits_the_first_backoff_after` | 4.568 | 4.628 | 4.712 |
| 17 | `ariadne-daemon::it` | `pull_request_babysit::stopping_review_keeps_a_dependabot_keeper` | 4.227 | 4.627 | 4.982 |
| 18 | `ariadne-daemon::it` | `pull_request_reviews::a_push_moves_the_worktree_and_is_told_once_with_the_last_reviewed_sha` | 4.327 | 4.594 | 5.293 |
| 19 | `ariadne-daemon::it` | `kept_requests::the_daemon_promotes_only_its_own_checked_draft_once_per_head` | 4.297 | 4.541 | 4.738 |
| 20 | `ariadne-daemon::it` | `pull_request_reviews::an_asked_review_starts_on_a_draft_and_outlives_its_draft_state` | 4.395 | 4.537 | 4.745 |
| 21 | `ariadne-daemon::it` | `hosts::a_source_build_daemon_builds_its_commit_on_the_new_machine` | 4.293 | 4.486 | 4.853 |
| 22 | `ariadne-daemon::it` | `kept_requests::a_check_that_recovers_and_fails_again_is_told_again` | 4.320 | 4.419 | 4.694 |
| 23 | `ariadne-daemon::it` | `hosts::selected_credentials_cross_ssh_without_entering_progress_and_retry_skips_the_copy` | 4.072 | 4.343 | 4.460 |
| 24 | `ariadne-daemon::it` | `pull_request_reviews::a_submitted_review_stores_the_checkout_head_when_the_forge_head_moved` | 4.183 | 4.331 | 4.482 |
| 25 | `ariadne-daemon::it` | `pull_request_reviews::a_review_resolves_its_own_fixed_finding` | 4.011 | 4.261 | 4.677 |
| 26 | `ariadne-daemon::it` | `hosts::a_missing_compiler_is_manual_and_a_retry_does_not_build_the_same_commit_again` | 4.142 | 4.254 | 4.840 |
| 27 | `ariadne-daemon::it` | `workflow_pull_request::the_pr_column_opens_the_request_once_and_keeps_it` | 3.997 | 4.050 | 4.864 |
| 28 | `ariadne-daemon::it` | `hosts::an_unusable_cargo_without_curl_needs_curl_and_a_resume_installs_rust` | 3.773 | 3.983 | 4.337 |
| 29 | `ariadne-daemon::it` | `pull_request_reviews::a_request_the_forge_stops_listing_after_my_review_keeps_its_review` | 3.712 | 3.960 | 4.113 |
| 30 | `ariadne-daemon::it` | `kept_requests::the_news_of_its_request_reaches_the_pr_agent_once` | 3.736 | 3.885 | 3.976 |

## Desktop tests

| Rank | Binary or file | Test | Min s | Median s | Max s |
| ---: | --- | --- | ---: | ---: | ---: |
| 1 | `src/features/sessions/console/status.test.tsx` | `shows only the first line of a multiline tool name, whole on hover, with the tokens at the right end` | 0.867 | 0.883 | 0.901 |
| 2 | `src/features/pull-requests/pull-request-panel.test.tsx` | `starts and stops a review of a Dependabot request without a review assignment` | 0.750 | 0.785 | 0.826 |
| 3 | `src/features/goals/hosts.test.tsx` | `clears the model and the effort when the goal dialog's host changes` | 0.759 | 0.781 | 0.931 |
| 4 | `src/features/system/daemon-logs-drawer.test.tsx` | `opens from the footer, tails the stream, and lets go of it on close` | 0.682 | 0.737 | 0.760 |
| 5 | `src/features/agents/agents-page.test.tsx` | `ranking a model > sends each rank as its lowercase word, with the id in the body` | 0.685 | 0.736 | 0.786 |
| 6 | `src/features/permissions/permissions-hosts.test.tsx` | `shows each host's own device and writes a device change to that host alone` | 0.669 | 0.704 | 0.806 |
| 7 | `src/features/command-palette/command-palette.test.tsx` | `replaces the open goal with a task picked from the palette, and Escape empties the pane rather than reopening the goal` | 0.643 | 0.692 | 0.743 |
| 8 | `src/features/hosts/host-edit.test.tsx` | `adds a host with name, address, colour and an Advanced remote socket, and shows its row through host_updated` | 0.649 | 0.686 | 0.725 |
| 9 | `src/features/forge/forge-hosts.test.tsx` | `opens the new session on the host Start review picked, not on the panel's host` | 0.607 | 0.664 | 0.756 |
| 10 | `src/features/sessions/session-actions.test.tsx` | `opens the switch dialog, posts its pin, and follows a successor` | 0.590 | 0.639 | 0.674 |
| 11 | `src/features/permissions/ai-card.test.tsx` | `draws the marker and the label from a test result` | 0.610 | 0.627 | 0.681 |
| 12 | `src/features/tasks/hosts.test.tsx` | `reads the task of a named host and refreshes its messages on that host's stream` | 0.588 | 0.625 | 0.641 |
| 13 | `src/features/pull-requests/hosts.test.tsx` | `opens the new review console on box` | 0.587 | 0.615 | 0.707 |
| 14 | `src/features/goals/hosts.test.tsx` | `names the host in the toast of a goal created on a named host, and offers all hosts when the pick leaves it out` | 0.571 | 0.613 | 0.647 |
| 15 | `src/components/pane-focus.test.tsx` | `keeps focus in the task pane after opening a goal from the board` | 0.574 | 0.611 | 0.668 |
| 16 | `src/features/system/daemon-logs-drawer.test.tsx` | `reconnects with a service and level filter when one is picked` | 0.544 | 0.599 | 0.657 |
| 17 | `src/features/command-palette/hosts.test.tsx` | `lists the tasks of every selected host, and opens a row on its own host` | 0.575 | 0.593 | 0.609 |
| 18 | `src/features/pull-requests/pull-requests-page.test.tsx` | `renders rows, updates from an event, and opens the floating panel` | 0.527 | 0.568 | 0.639 |
| 19 | `src/features/forge/forge-hosts.test.tsx` | `merges the pull requests of every host under All, and opens a panel on its row's host` | 0.503 | 0.558 | 0.581 |
| 20 | `src/features/issues/issues-page.test.tsx` | `lists forge issues and fills the goal dialog from a chosen issue` | 0.495 | 0.544 | 0.558 |

## Dashboard tests

| Rank | Binary or file | Test | Min s | Median s | Max s |
| ---: | --- | --- | ---: | ---: | ---: |
| 1 | `src/page.test.tsx` | `Page > remounts ViewDashboard with the fresh document on a reload message` | 0.063 | 0.064 | 0.064 |
| 2 | `src/page.test.tsx` | `Page > opens the dashboard in view mode, not Perses's own editing mode` | 0.051 | 0.055 | 0.056 |
| 3 | `src/dashboard-client.test.ts` | `getDashboard > reads the document off GET /v1/dashboards/{name}` | 0.029 | 0.031 | 0.036 |
| 4 | `src/use-host-messages.test.ts` | `useHostMessages > calls onTheme for a same-origin theme message` | 0.015 | 0.018 | 0.021 |
| 5 | `src/format-value.test.ts` | `formatValue > still formats a real value` | 0.015 | 0.016 | 0.018 |

## Individual observations

| Series | Stage | Condition | Rep | Wall s | CPU user s | CPU system s | Exit | Link calls | Link sum s | Link max s |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| a | bins | cold | 1 | 103.077 | 608.93 | 97.03 | 0 | 78 | 11.54 | 1.26 |
| a | bins | warm | 1 | 0.768 | 0.20 | 0.36 | 0 | 0 | 0.0 | 0.0 |
| a | bins | cold | 2 | 104.499 | 614.06 | 97.93 | 0 | 78 | 12.2 | 1.17 |
| a | bins | warm | 2 | 1.670 | 0.22 | 0.43 | 0 | 0 | 0.0 | 0.0 |
| a | bins | cold | 3 | 104.391 | 617.03 | 98.08 | 0 | 78 | 12.02 | 1.27 |
| a | bins | warm | 3 | 1.270 | 0.24 | 0.45 | 0 | 0 | 0.0 | 0.0 |
| a | testbuild | cold | 1 | 147.815 | 847.06 | 125.84 | 0 | 93 | 20.08 | 1.53 |
| a | nextest | cold | 1 | 79.101 | 719.12 | 265.65 | 0 | 0 | 0.0 | 0.0 |
| a | testbuild | warm | 1 | 1.068 | 0.52 | 0.77 | 0 | 0 | 0.0 | 0.0 |
| a | nextest | warm | 1 | 78.521 | 722.65 | 267.75 | 100 | 0 | 0.0 | 0.0 |
| a | testbuild | cold | 2 | 148.332 | 860.11 | 128.63 | 0 | 93 | 20.84 | 2.2 |
| a | nextest | cold | 2 | 78.130 | 714.36 | 263.82 | 100 | 0 | 0.0 | 0.0 |
| a | testbuild | warm | 2 | 1.019 | 0.50 | 0.75 | 0 | 0 | 0.0 | 0.0 |
| a | nextest | warm | 2 | 79.077 | 730.37 | 269.37 | 0 | 0 | 0.0 | 0.0 |
| a | testbuild | cold | 3 | 148.381 | 857.05 | 126.83 | 0 | 93 | 20.29 | 1.28 |
| a | nextest | cold | 3 | 79.142 | 722.25 | 266.90 | 0 | 0 | 0.0 | 0.0 |
| a | testbuild | warm | 3 | 1.069 | 0.52 | 0.76 | 0 | 0 | 0.0 | 0.0 |
| a | nextest | warm | 3 | 79.576 | 731.44 | 268.75 | 0 | 0 | 0.0 | 0.0 |
| a | doctest | cold | 1 | 99.346 | 564.72 | 89.25 | 0 | 69 | 9.1 | 0.59 |
| a | doctest | warm | 1 | 3.826 | 2.59 | 0.99 | 0 | 0 | 0.0 | 0.0 |
| a | doctest | cold | 2 | 98.791 | 559.50 | 89.16 | 0 | 69 | 9.38 | 0.42 |
| a | doctest | warm | 2 | 4.579 | 2.63 | 0.98 | 0 | 0 | 0.0 | 0.0 |
| a | doctest | cold | 3 | 97.082 | 553.53 | 87.43 | 0 | 69 | 9.08 | 0.55 |
| a | doctest | warm | 3 | 3.777 | 2.60 | 0.91 | 0 | 0 | 0.0 | 0.0 |
| b | testbuild | cold | 1 | 147.232 | 845.83 | 124.79 | 0 | 93 | 19.58 | 1.22 |
| b | nextest | cold | 1 | 79.191 | 722.80 | 267.51 | 100 | 0 | 0.0 | 0.0 |
| b | testbuild | warm | 1 | 1.019 | 0.46 | 0.77 | 0 | 0 | 0.0 | 0.0 |
| b | nextest | warm | 1 | 79.334 | 729.10 | 269.47 | 100 | 0 | 0.0 | 0.0 |
| b | testbuild | cold | 2 | 148.728 | 856.93 | 128.37 | 0 | 93 | 20.69 | 2.27 |
| b | nextest | cold | 2 | 79.685 | 725.43 | 268.43 | 0 | 0 | 0.0 | 0.0 |
| b | testbuild | warm | 2 | 1.069 | 0.52 | 0.78 | 0 | 0 | 0.0 | 0.0 |
| b | nextest | warm | 2 | 79.660 | 730.97 | 270.27 | 0 | 0 | 0.0 | 0.0 |
| b | testbuild | cold | 3 | 148.100 | 857.36 | 127.77 | 0 | 93 | 20.43 | 1.27 |
| b | nextest | cold | 3 | 79.726 | 725.38 | 268.58 | 0 | 0 | 0.0 | 0.0 |
| b | testbuild | warm | 3 | 1.119 | 0.57 | 0.73 | 0 | 0 | 0.0 | 0.0 |
| b | nextest | warm | 3 | 80.696 | 734.84 | 271.54 | 0 | 0 | 0.0 | 0.0 |
| a | clippy | cold | 1 | 93.884 | 564.78 | 92.52 | 0 | 76 | 11.11 | 0.57 |
| a | clippy | warm | 1 | 0.968 | 0.26 | 0.41 | 0 | 0 | 0.0 | 0.0 |
| a | clippy | cold | 2 | 90.055 | 555.08 | 91.51 | 0 | 76 | 10.1 | 0.54 |
| a | clippy | warm | 2 | 0.868 | 0.24 | 0.43 | 0 | 0 | 0.0 | 0.0 |
| a | clippy | cold | 3 | 90.501 | 555.43 | 91.17 | 0 | 76 | 9.93 | 0.57 |
| a | clippy | warm | 3 | 0.868 | 0.24 | 0.39 | 0 | 0 | 0.0 | 0.0 |
| a | tauri | cold | 1 | 60.246 | 324.46 | 62.99 | 0 | 93 | 13.84 | 0.52 |
| a | tauri | warm | 1 | 0.668 | 0.16 | 0.31 | 0 | 0 | 0.0 | 0.0 |
| a | tauri | cold | 2 | 58.990 | 321.02 | 62.76 | 0 | 93 | 13.54 | 0.42 |
| a | tauri | warm | 2 | 0.718 | 0.19 | 0.33 | 0 | 0 | 0.0 | 0.0 |
| a | tauri | cold | 3 | 58.491 | 323.77 | 62.72 | 0 | 93 | 14.08 | 0.44 |
| a | tauri | warm | 3 | 0.467 | 0.16 | 0.25 | 0 | 0 | 0.0 | 0.0 |
| a | desktop-ci | cold | 1 | 5.729 | 6.13 | 5.88 | 0 |  |  |  |
| a | dashboard-ci | cold | 1 | 14.907 | 10.91 | 11.48 | 0 |  |  |  |
| a | desktop-test | cold | 1 | 113.729 | 472.25 | 94.81 | 0 |  |  |  |
| a | dashboard-test | cold | 1 | 5.288 | 11.76 | 3.48 | 0 |  |  |  |
| a | build | cold | 1 | 29.758 | 52.71 | 18.09 | 0 |  |  |  |
| a | desktop-test | warm | 1 | 108.673 | 463.45 | 90.92 | 0 |  |  |  |
| a | dashboard-test | warm | 1 | 5.280 | 11.19 | 3.20 | 0 |  |  |  |
| a | build | warm | 1 | 28.270 | 53.08 | 17.38 | 0 |  |  |  |
| a | desktop-ci | cold | 2 | 6.035 | 6.53 | 6.02 | 0 |  |  |  |
| a | dashboard-ci | cold | 2 | 14.206 | 10.62 | 11.30 | 0 |  |  |  |
| a | desktop-test | cold | 2 | 113.531 | 472.55 | 95.09 | 0 |  |  |  |
| a | dashboard-test | cold | 2 | 5.330 | 11.82 | 3.55 | 0 |  |  |  |
| a | build | cold | 2 | 30.061 | 53.40 | 18.33 | 0 |  |  |  |
| a | desktop-test | warm | 2 | 108.460 | 463.51 | 90.77 | 0 |  |  |  |
| a | dashboard-test | warm | 2 | 5.329 | 11.02 | 3.27 | 0 |  |  |  |
| a | build | warm | 2 | 29.961 | 53.70 | 17.56 | 0 |  |  |  |
| a | desktop-ci | cold | 3 | 5.729 | 6.21 | 5.86 | 0 |  |  |  |
| a | dashboard-ci | cold | 3 | 10.596 | 10.64 | 11.46 | 0 |  |  |  |
| a | desktop-test | cold | 3 | 113.732 | 471.24 | 94.20 | 0 |  |  |  |
| a | dashboard-test | cold | 3 | 5.831 | 11.96 | 3.61 | 0 |  |  |  |
| a | build | cold | 3 | 30.819 | 53.21 | 19.30 | 0 |  |  |  |
| a | desktop-test | warm | 3 | 108.114 | 459.59 | 89.31 | 0 |  |  |  |
| a | dashboard-test | warm | 3 | 5.881 | 11.61 | 3.37 | 0 |  |  |  |
| a | build | warm | 3 | 29.312 | 53.20 | 17.79 | 0 |  |  |  |

## Background load before each run

| Series | Stage | Condition | Rep | Start UTC | Load 1/5/15 min | MemAvailable GiB | First `ps --sort=-pcpu` row (pid user %cpu %mem etimes comm) |
| --- | --- | --- | ---: | --- | --- | ---: | --- |
| a | bins | cold | 1 | 2026-11-23T21:28:36Z | 4.43/6.63/5.32 | 31.2 | `18828 malvarez 0.0 0.6 4107092352 opencode` |
| a | bins | warm | 1 | 2026-11-23T21:30:19Z | 6.89/7.76/5.93 | 31.2 | `18828 malvarez 0.0 0.6 4107092448 opencode` |
| a | bins | cold | 2 | 2026-11-23T21:30:20Z | 6.89/7.76/5.93 | 31.2 | `18828 malvarez 0.0 0.6 4107092448 opencode` |
| a | bins | warm | 2 | 2026-11-23T21:32:05Z | 8.74/9.00/6.63 | 31.2 | `18828 malvarez 0.0 0.6 4107092576 opencode` |
| a | bins | cold | 3 | 2026-11-23T21:32:07Z | 8.74/9.00/6.63 | 31.2 | `18828 malvarez 0.0 0.6 4107092576 opencode` |
| a | bins | warm | 3 | 2026-11-23T21:33:52Z | 8.27/9.70/7.20 | 31.2 | `18828 malvarez 0.0 0.6 4107092672 opencode` |
| a | testbuild | cold | 1 | 2026-11-23T21:34:04Z | 6.59/9.26/7.09 | 31.2 | `18828 malvarez 0.0 0.6 4107092672 opencode` |
| a | nextest | cold | 1 | 2026-11-23T21:36:36Z | 5.90/9.31/7.54 | 31.2 | `18828 malvarez 0.0 0.6 4107092832 opencode` |
| a | testbuild | warm | 1 | 2026-11-23T21:38:03Z | 20.34/13.45/9.17 | 31.2 | `18828 malvarez 0.0 0.6 4107092896 opencode` |
| a | nextest | warm | 1 | 2026-11-23T21:38:04Z | 19.20/13.32/9.16 | 31.2 | `18828 malvarez 0.0 0.6 4107092896 opencode` |
| a | testbuild | cold | 2 | 2026-11-23T21:39:22Z | 25.45/16.60/10.63 | 31.2 | `18828 malvarez 0.0 0.6 4107092992 opencode` |
| a | nextest | cold | 2 | 2026-11-23T21:42:04Z | 7.27/13.66/10.60 | 31.2 | `18828 malvarez 0.0 0.6 4107093152 opencode` |
| a | testbuild | warm | 2 | 2026-11-23T21:43:22Z | 21.09/16.51/11.83 | 31.2 | `18828 malvarez 0.0 0.6 4107093248 opencode` |
| a | nextest | warm | 2 | 2026-11-23T21:43:23Z | 21.09/16.51/11.83 | 31.2 | `18828 malvarez 0.0 0.6 4107093248 opencode` |
| a | testbuild | cold | 3 | 2026-11-23T21:44:49Z | 21.01/17.99/12.80 | 31.2 | `18828 malvarez 0.0 0.6 4107093312 opencode` |
| a | nextest | cold | 3 | 2026-11-23T21:47:17Z | 8.45/15.14/12.58 | 31.2 | `18828 malvarez 0.0 0.6 4107093472 opencode` |
| a | testbuild | warm | 3 | 2026-11-23T21:48:36Z | 26.81/18.88/14.07 | 31.2 | `18828 malvarez 0.0 0.6 4107093536 opencode` |
| a | nextest | warm | 3 | 2026-11-23T21:48:38Z | 26.81/18.88/14.07 | 31.2 | `18828 malvarez 0.0 0.6 4107093536 opencode` |
| a | doctest | cold | 1 | 2026-11-23T21:50:09Z | 22.54/20.22/15.05 | 31.2 | `18828 malvarez 0.0 0.6 4107093632 opencode` |
| a | doctest | warm | 1 | 2026-11-23T21:51:49Z | 10.82/17.53/14.66 | 31.2 | `18828 malvarez 0.0 0.6 4107093760 opencode` |
| a | doctest | cold | 2 | 2026-11-23T21:51:52Z | 10.82/17.53/14.66 | 31.2 | `18828 malvarez 0.0 0.6 4107093760 opencode` |
| a | doctest | warm | 2 | 2026-11-23T21:53:31Z | 7.35/15.10/14.13 | 31.2 | `18828 malvarez 0.0 0.6 4107093856 opencode` |
| a | doctest | cold | 3 | 2026-11-23T21:54:09Z | 3.81/13.22/13.54 | 31.2 | `18828 malvarez 0.0 0.6 4107093888 opencode` |
| a | doctest | warm | 3 | 2026-11-23T21:55:46Z | 7.40/12.55/13.31 | 31.2 | `18828 malvarez 0.0 0.6 4107093984 opencode` |
| b | testbuild | cold | 1 | 2026-11-23T21:56:04Z | 5.36/11.75/13.03 | 31.2 | `18828 malvarez 0.0 0.6 4107093984 opencode` |
| b | nextest | cold | 1 | 2026-11-23T21:58:31Z | 7.20/11.66/12.93 | 31.2 | `18828 malvarez 0.0 0.6 4107094144 opencode` |
| b | testbuild | warm | 1 | 2026-11-23T21:59:50Z | 24.92/16.02/14.34 | 31.2 | `18828 malvarez 0.0 0.6 4107094208 opencode` |
| b | nextest | warm | 1 | 2026-11-23T21:59:52Z | 24.92/16.02/14.34 | 31.2 | `18828 malvarez 0.0 0.6 4107094208 opencode` |
| b | testbuild | cold | 2 | 2026-11-23T22:01:15Z | 25.39/18.50/15.37 | 31.2 | `18828 malvarez 0.0 0.6 4107094304 opencode` |
| b | nextest | cold | 2 | 2026-11-23T22:03:44Z | 10.31/16.05/15.00 | 31.2 | `18828 malvarez 0.0 0.6 4107094464 opencode` |
| b | testbuild | warm | 2 | 2026-11-23T22:05:03Z | 22.51/18.42/15.91 | 31.2 | `18828 malvarez 0.0 0.6 4107094560 opencode` |
| b | nextest | warm | 2 | 2026-11-23T22:05:04Z | 20.87/18.15/15.83 | 31.2 | `18828 malvarez 0.0 0.6 4107094560 opencode` |
| b | testbuild | cold | 3 | 2026-11-23T22:06:28Z | 28.47/21.20/17.10 | 31.2 | `18828 malvarez 0.0 0.6 4107094624 opencode` |
| b | nextest | cold | 3 | 2026-11-23T22:08:56Z | 9.19/17.30/16.35 | 31.2 | `18828 malvarez 0.0 0.6 4107094784 opencode` |
| b | testbuild | warm | 3 | 2026-11-23T22:10:16Z | 27.33/20.90/17.68 | 31.2 | `18828 malvarez 0.0 0.6 4107094848 opencode` |
| b | nextest | warm | 3 | 2026-11-23T22:10:17Z | 27.33/20.90/17.68 | 31.2 | `18828 malvarez 0.0 0.6 4107094848 opencode` |
| a | clippy | cold | 1 | 2026-11-23T22:12:01Z | 23.20/22.29/18.58 | 31.1 | `18828 malvarez 0.0 0.6 4107094944 opencode` |
| a | clippy | warm | 1 | 2026-11-23T22:13:35Z | 12.53/19.76/18.09 | 31.1 | `18828 malvarez 0.0 0.6 4107095072 opencode` |
| a | clippy | cold | 2 | 2026-11-23T22:13:36Z | 12.53/19.76/18.09 | 31.1 | `18828 malvarez 0.0 0.6 4107095072 opencode` |
| a | clippy | warm | 2 | 2026-11-23T22:15:06Z | 10.38/18.01/17.68 | 31.1 | `18828 malvarez 0.0 0.6 4107095136 opencode` |
| a | clippy | cold | 3 | 2026-11-23T22:15:07Z | 10.38/18.01/17.68 | 31.1 | `18828 malvarez 0.0 0.6 4107095136 opencode` |
| a | clippy | warm | 3 | 2026-11-23T22:16:37Z | 9.15/16.36/17.17 | 31.1 | `18828 malvarez 0.0 0.6 4107095232 opencode` |
| a | tauri | cold | 1 | 2026-11-23T22:16:45Z | 7.90/15.85/17.00 | 31.1 | `18828 malvarez 0.0 0.6 4107095232 opencode` |
| a | tauri | warm | 1 | 2026-11-23T22:17:50Z | 8.22/14.79/16.57 | 31.1 | `18828 malvarez 0.0 0.6 4107095296 opencode` |
| a | tauri | cold | 2 | 2026-11-23T22:17:51Z | 8.22/14.79/16.57 | 31.1 | `18828 malvarez 0.0 0.6 4107095296 opencode` |
| a | tauri | warm | 2 | 2026-11-23T22:18:50Z | 8.41/13.99/16.21 | 31.1 | `18828 malvarez 0.0 0.6 4107095360 opencode` |
| a | tauri | cold | 3 | 2026-11-23T22:18:51Z | 8.41/13.99/16.21 | 31.1 | `18828 malvarez 0.0 0.6 4107095360 opencode` |
| a | tauri | warm | 3 | 2026-11-23T22:19:50Z | 9.38/13.61/15.96 | 31.1 | `18828 malvarez 0.0 0.6 4107095424 opencode` |
| a | desktop-ci | cold | 1 | 2026-11-23T22:19:58Z | 8.63/13.38/15.88 | 31.1 | `18828 malvarez 0.0 0.6 4107095424 opencode` |
| a | dashboard-ci | cold | 1 | 2026-11-23T22:20:04Z | 8.58/13.29/15.83 | 31.1 | `18828 malvarez 0.0 0.6 4107095424 opencode` |
| a | desktop-test | cold | 1 | 2026-11-23T22:20:19Z | 8.96/13.12/15.73 | 31.1 | `18828 malvarez 0.0 0.6 4107095456 opencode` |
| a | dashboard-test | cold | 1 | 2026-11-23T22:22:13Z | 6.13/10.73/14.55 | 31.1 | `18828 malvarez 0.0 0.6 4107095584 opencode` |
| a | build | cold | 1 | 2026-11-23T22:22:18Z | 6.20/10.67/14.51 | 31.1 | `18828 malvarez 0.0 0.6 4107095584 opencode` |
| a | desktop-test | warm | 1 | 2026-11-23T22:22:52Z | 5.35/9.96/14.13 | 31.1 | `18828 malvarez 0.0 0.6 4107095616 opencode` |
| a | dashboard-test | warm | 1 | 2026-11-23T22:24:41Z | 6.79/8.93/13.28 | 31.1 | `18828 malvarez 0.0 0.6 4107095712 opencode` |
| a | build | warm | 1 | 2026-11-23T22:24:47Z | 6.89/8.91/13.25 | 31.1 | `18828 malvarez 0.0 0.6 4107095712 opencode` |
| a | desktop-ci | cold | 2 | 2026-11-23T22:25:15Z | 6.26/8.57/13.00 | 31.1 | `18828 malvarez 0.0 0.6 4107095744 opencode` |
| a | dashboard-ci | cold | 2 | 2026-11-23T22:25:21Z | 6.16/8.51/12.96 | 31.1 | `18828 malvarez 0.0 0.6 4107095744 opencode` |
| a | desktop-test | cold | 2 | 2026-11-23T22:25:36Z | 6.02/8.37/12.84 | 31.1 | `18828 malvarez 0.0 0.6 4107095776 opencode` |
| a | dashboard-test | cold | 2 | 2026-11-23T22:27:29Z | 6.51/7.76/12.09 | 31.1 | `18828 malvarez 0.0 0.6 4107095872 opencode` |
| a | build | cold | 2 | 2026-11-23T22:27:35Z | 6.23/7.68/12.04 | 31.1 | `18828 malvarez 0.0 0.6 4107095904 opencode` |
| a | desktop-test | warm | 2 | 2026-11-23T22:28:08Z | 4.44/7.11/11.71 | 31.1 | `18828 malvarez 0.0 0.6 4107095936 opencode` |
| a | dashboard-test | warm | 2 | 2026-11-23T22:29:57Z | 7.05/7.22/11.23 | 31.1 | `18828 malvarez 0.0 0.6 4107096032 opencode` |
| a | build | warm | 2 | 2026-11-23T22:30:02Z | 6.81/7.17/11.19 | 31.1 | `18828 malvarez 0.0 0.6 4107096032 opencode` |
| a | desktop-ci | cold | 3 | 2026-11-23T22:30:32Z | 9.90/7.85/11.28 | 31.1 | `18828 malvarez 0.0 0.6 4107096064 opencode` |
| a | dashboard-ci | cold | 3 | 2026-11-23T22:30:38Z | 9.18/7.73/11.23 | 31.1 | `18828 malvarez 0.0 0.6 4107096064 opencode` |
| a | desktop-test | cold | 3 | 2026-11-23T22:30:49Z | 8.39/7.61/11.15 | 31.1 | `18828 malvarez 0.0 0.6 4107096096 opencode` |
| a | dashboard-test | cold | 3 | 2026-11-23T22:32:43Z | 6.56/7.13/10.55 | 31.1 | `18828 malvarez 0.0 0.6 4107096192 opencode` |
| a | build | cold | 3 | 2026-11-23T22:32:49Z | 6.68/7.14/10.54 | 31.1 | `18828 malvarez 0.0 0.6 4107096192 opencode` |
| a | desktop-test | warm | 3 | 2026-11-23T22:33:24Z | 4.62/6.57/10.22 | 31.1 | `18828 malvarez 0.0 0.6 4107096256 opencode` |
| a | dashboard-test | warm | 3 | 2026-11-23T22:35:12Z | 6.34/6.52/9.79 | 31.1 | `18828 malvarez 0.0 0.6 4107096352 opencode` |
| a | build | warm | 3 | 2026-11-23T22:35:18Z | 6.39/6.53/9.77 | 31.1 | `18828 malvarez 0.0 0.6 4107096352 opencode` |
