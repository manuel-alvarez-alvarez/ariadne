# Host malvarez: Linux build and test latency

These results come from host malvarez only.
Do not pool them with the GitHub runner data or the macOS data in [the main report](README.md).

The reader selects the next latency fix tasks.
This page gives the stage times of the complete check chain and the ranked slow tests.

## Result

- Cold test compilation is the slowest Rust stage: 147–149 s.
- The nextest run takes 78–81 s, cold or warm, so warm artifacts do not make it faster.
- The desktop suite takes 108–114 s, and it is the slowest stage with warm artifacts.
- The nextest run is throughput-bound: in each run, the test durations sum to 1239–1270 s on 16 threads.
- The ranking uses the median of six runs per test. The median durations sum to 1244 s.
- The 20 slowest Rust tests hold 9.5 % of that median sum. The 342 tests with a median of 1 s or more hold 60 %.
- No Rust test reached the 10 s `SLOW` mark in any run.
- The highest median is 8.48 s, for rank 1. The longest single observation is 9.11 s, for rank 2.
- Three daemon tests failed intermittently. See [failures](linux-host/failures.md).

## Host and revision

All runs used main at `226a47eb63c8ab763167b3fbedc89e52a0e66ab3`.
The revision was pinned once, and the worktree stayed clean except for the evidence directory.
Measurements ran on 2026-11-23 from 21:28 to 22:37 UTC.
[`environment.txt`](linux-host/environment.txt) gives the full record.

| Item | Value |
| --- | --- |
| Host label | malvarez (hostname `ariadne`) |
| Hardware | AMD Ryzen 7 5700U, 8 cores, 16 threads, 32 GiB, no swap |
| Kernel | Linux 7.0.2-4-pve, in an LXC container, no cgroup CPU or memory limit |
| Distribution | Ubuntu 25.04 |
| Filesystem | ZFS for the worktree and targets; tmpfs for `/tmp` |
| Rust | rustc and cargo 1.100.0, LLVM 23.1.5, clippy 0.1.100 |
| Test runner | cargo-nextest 0.9.152 |
| Linker | GNU cc 14.2.0, GNU ld 2.44 |
| Frontend | Node 26.13.1, npm 11.20.0 |
| Other | Python 3.13.3, git 2.48.1, tmux 3.5a, webkit2gtk-4.1 2.50.4 |

Before the run, `CARGO_HOME`, `CARGO_TARGET_DIR`, `RUSTC_WRAPPER`, `CARGO_INCREMENTAL`, `RUSTFLAGS`, `SCCACHE_DIR`, `npm_config_cache` and `CI` were unset.
No `~/.cargo/config.toml` existed.

## Method

The method follows [the macOS method](reproduction.md), with these differences:

- Three cold and three warm observations per stage, not two.
- [`run.py`](linux-host/run.py) runs one stage per call. [`summarize.py`](linux-host/summarize.py) makes the tables.
- The linker wrapper is `/usr/bin/time -p` around `/usr/bin/cc`, set through `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER`.
- The wrapper file is named `cc`. Under another name, rustc passes ld flags to it and the link fails.

Private directories, all under the worktree's ignored `target/latency-host`:

- `cargo-home`: a copy of `~/.cargo/registry` and `~/.cargo/git`, then `cargo fetch --locked` for both workspaces.
- `npm-cache`: a copy of `~/.npm`.
- `target-<family>-<rep>`: one empty target for each cold observation.
- `frontend-<rep>`: a copy of `ui/` without `node_modules`, `dist`, `target`, `*.tsbuildinfo` and `junit.xml`. A `crates` link beside it serves the dashboard's JSON import.

The driver passes only `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG` and its own variables to a command.
`CARGO_INCREMENTAL` stays unset, as in local development.
Frontend commands get `CI=true`, because both Vite configurations write JUnit only then.
The shared caches were only read. No shared cache was cleared or changed.

Conditions:

- Cold Rust: the first run of the command on an empty private target, with warm download caches.
- Warm Rust: the same command, again, on the same target.
- Cold nextest: the first execution after the cold test compilation.
- Warm nextest: the second execution on the same target.
- Cold frontend: the first run after `npm ci` in a new snapshot. Warm: the second run in that snapshot.
- No run flushed the OS page cache. Cold means cold build artifacts, not cold disks.

Each command ran under `timeout -k 10 560` in a new process group.
The driver ended the group after each command, and then killed each process that carried its marker variable.
No command reached its limit, and no command left a process.
The stages ran one at a time. The task started no other work during a run.

## Stage times

Times are wall seconds as min / median / max.
[`tables.md`](linux-host/tables.md) gives each observation with its CPU time, linker data and the load before it.

| Stage | Command (cwd) | Cold | Warm |
| --- | --- | ---: | ---: |
| Workspace binary build | `cargo build --locked --workspace --bins` | 103.1 / 104.4 / 104.5 | 0.77 / 1.27 / 1.67 |
| Test compilation | `cargo nextest run --workspace --profile ci --no-run` | 147.2 / 148.1 / 148.7 | 1.02 / 1.07 / 1.12 |
| Nextest execution, command | `cargo nextest run --workspace --profile ci --status-level fail --final-status-level fail` | 79.2 / 79.7 / 79.7 | 79.3 / 79.7 / 80.7 |
| Nextest execution, runner `Summary` | same | 77.5 / 78.0 / 78.1 | 78.1 / 78.5 / 79.5 |
| Doctests | `cargo test --doc --workspace` | 97.1 / 98.8 / 99.3 | 3.78 / 3.83 / 4.58 |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 90.1 / 90.5 / 93.9 | 0.87 / 0.87 / 0.97 |
| Tauri check | `cargo check` (`ui/src-tauri`) | 58.5 / 59.0 / 60.2 | 0.47 / 0.67 / 0.72 |
| Desktop dependencies | `npm ci` (`ui`) | 5.73 / 5.73 / 6.04 | not measured |
| Dashboard dependencies | `npm ci` (`ui/dashboard`) | 10.6 / 14.2 / 14.9 | not measured |
| Desktop tests | `npm test` (`ui`) | 113.5 / 113.7 / 113.7 | 108.1 / 108.5 / 108.7 |
| Dashboard tests | `npm test` (`ui/dashboard`) | 5.29 / 5.33 / 5.83 | 5.28 / 5.33 / 5.88 |
| Frontend build | `npm run build` (`ui`) | 29.8 / 30.1 / 30.8 | 28.3 / 29.3 / 30.0 |

Compilation and execution are separate rows.
The nextest command row includes a 0.5 s fingerprint check before the runner starts.
The warm build rows are fingerprint checks with no compilation.

Linker-driver data for the cold rows, from the wrapper:

| Stage | Calls | Sum of driver seconds | Longest call s |
| --- | ---: | ---: | ---: |
| Binary build | 78 | 11.5–12.2 | 1.17–1.27 |
| Test compilation | 93 | 19.6–20.8 | 1.22–2.27 |
| Doctests | 69 | 9.1–9.4 | 0.42–0.59 |
| Clippy | 76 | 9.9–11.1 | 0.54–0.57 |
| Tauri check | 93 | 13.5–14.1 | 0.42–0.52 |

Linker calls overlap with compilation, so do not subtract their sum from the wall time.
The doctest count excludes the links that rustdoc makes for each doctest.

CPU use, from the child rusage of each command:

- Cold test compilation used 846–860 s of user CPU in 148 s: about 5.8 of 16 threads.
- Nextest execution used 714–735 s of user CPU in 79 s: about 9.2 of 16 threads.
- The desktop suite used 460–473 s of user CPU in 108–114 s: about 4.2 threads, near its `maxWorkers: 4`.

## Slowest Rust tests

The ranking uses the JUnit `time` of series b: three cold and three warm nextest runs.
The rank order is by the median of the six runs, so a test with a higher maximum can rank lower.
All 2651 tests in 15 binaries ran in each run. [`rust-test-durations.csv`](linux-host/rust-test-durations.csv) gives every test.
The `ariadne-daemon::it` binary holds 1154 tests and 1146 s of the 1244 s median sum.

The cause column comes from the test source. A wait is a daemon timeout, budget or backoff that runs out.
A timer is a fixed sleep or delay. Polling is a daemon interval that the test waits through.

| Rank | Test (`ariadne-daemon::it`) | Min / median / max s | Cause | Second cause | Evidence |
| ---: | --- | --- | --- | --- | --- |
| 1 | `pull_request_babysit::asking_keeps_another_authors_request_until_the_merge` | 7.471 / 8.483 / 8.610 | process start | real work | 4 `fetch_again` rounds (`pull_request_babysit.rs:160`) and 7 more waits on `stub.fetches()` in `keep_until_merge` (`:213`), one keeper ACP session, git push and teardown. 3 × `QUIET` add 0.3 s. |
| 2 | `pull_request_babysit::asking_keeps_a_request_of_mine_by_hand_until_the_merge` | 7.889 / 8.457 / 9.114 | process start | real work | Same body as rank 1 (`keep_until_merge`, `pull_request_babysit.rs:213`). |
| 3 | `pull_request_babysit::asking_keeps_a_dependabot_request_until_the_merge` | 7.962 / 8.380 / 8.992 | process start | real work | Same body as rank 1. |
| 4 | `pull_request_reviews::a_review_round_needs_an_ask_and_runs_on_the_current_head` | 7.992 / 8.226 / 8.539 | process start | real work | 7 `fetch_again` rounds (`pull_request_reviews.rs:277`) after the first fetch and the fetch of the ask, 5 reviewer turns, 2 git pushes, 1 × `QUIET`. |
| 5 | `pull_request_reviews::a_reply_in_a_review_thread_starts_one_round` | 6.082 / 6.637 / 7.440 | process start | — | About 6 forge fetches and a review post; 2 × `QUIET`. |
| 6 | `hosts::updates_wait_for_idle_and_retry_without_replacing_installed_binaries` | 6.051 / 6.473 / 6.745 | process start | — | 3 host update runs, each a chain of stub `ssh` calls (`common/ssh.rs:162`; `src/hosts/setup.rs` has 11 `self.ssh` call sites). The maintenance poll is 100 ms. No long sleep found. |
| 7 | `doctor::doctor_reports_a_missing_forced_place_and_its_reason` | 5.915 / 6.091 / 6.487 | wait | — | A silent herdr server costs one read timeout of `Timeouts::default().probe` = 5 s (`src/place.rs:373`, `src/timeouts.rs:181`). The value is read from the default, so the harness cannot shorten it. |
| 8 | `kept_requests::an_ariadne_review_of_a_kept_request_reaches_its_pr_agent` | 5.783 / 5.992 / 6.262 | process start | fixture | `kept_request_running` fixture (`kept_requests.rs:333`) with a `pr` ACP agent, then about 7 forge fetches and a second ACP session. |
| 9 | `pull_request_babysit::keeper_push_and_external_push_reach_the_reviewer_independently` | 5.511 / 5.937 / 6.814 | process start | fixture | `both_seats_script` fixture (`pull_request_babysit.rs:495`): 2 ACP sessions, about 5 forge fetches. |
| 10 | `place::a_herdr_server_that_does_not_answer_is_not_found` | 5.144 / 5.449 / 5.487 | wait | — | One 5 s herdr `probe` read timeout, as rank 7 (`src/place.rs:373`). |
| 11 | `pull_request_reviews::a_forge_re_request_round_waits_for_the_read_of_the_new_head` | 5.080 / 5.336 / 5.867 | process start | — | About 5 forge fetches and 1 push. A held stub call checks its gate every 20 ms (`common/forge.rs:276`). |
| 12 | `hosts::daemon_version_installs_the_recorded_release_of_a_release_daemon` | 4.700 / 5.059 / 5.351 | process start | — | 2 host setups and 1 update, each a chain of stub `ssh` and `gh` calls (`hosts.rs:985`). |
| 13 | `pull_request_babysit::stopping_babysitting_keeps_a_dependabot_reviewer` | 4.506 / 4.872 / 5.067 | process start | fixture | `both_seats_script` fixture, then 2 stops with session cancel and git teardown. |
| 14 | `pull_request_reviews::a_push_reaches_an_idle_session_without_a_wait` | 4.618 / 4.822 / 5.216 | process start | — | About 4 forge fetches, 2 git commits, 2 news turns. No timer. |
| 15 | `pull_request_babysit::a_keeper_whose_agent_dies_the_moment_it_starts_gives_up_after_the_budget` | 4.301 / 4.822 / 5.293 | wait | polling | 3 dead launches (`SPAWN_RETRY_BUDGET`, `src/scheduler/mod.rs:99`), each seen on a 0.5 s `full_reconcile` tick (`RUNS_OUT`). `..Timeouts::default()` also restores `forge_wake_debounce` = 5 s (`src/timeouts.rs:212`); its effect is not proven. |
| 16 | `host_reconnect::a_down_forward_starts_again_at_once_and_waits_the_first_backoff_after` | 4.568 / 4.628 / 4.712 | wait | — | Sits through the `ssh_retry` backoff by design: 1 s + 2 s, then 1 s (`host_reconnect.rs:48`). |
| 17 | `pull_request_babysit::stopping_review_keeps_a_dependabot_keeper` | 4.227 / 4.627 / 4.982 | process start | fixture | As rank 13, with the stops in the other order. |
| 18 | `pull_request_reviews::a_push_moves_the_worktree_and_is_told_once_with_the_last_reviewed_sha` | 4.327 / 4.594 / 5.293 | process start | — | About 4 forge fetches, 1 review post, 1 push, 1 × `QUIET`. |
| 19 | `kept_requests::the_daemon_promotes_only_its_own_checked_draft_once_per_head` | 4.297 / 4.541 / 4.738 | fixture | process start | Builds the `kept_request_running` fixture twice, once per author (`kept_requests.rs:2449`). |
| 20 | `pull_request_reviews::an_asked_review_starts_on_a_draft_and_outlives_its_draft_state` | 4.395 / 4.537 / 4.745 | process start | — | About 5 forge fetches and 1 reviewer session. No timer. |

Causes shared by these tests:

- **Sequential stub forge fetches**, in 14 of the 20 tests.
  The test waits for each completed forge round (`fetch_again`).
  One round makes about 10 stub `gh` calls in 4–5 sequential steps (`src/forge/poll.rs:543`), and each call starts `sh` and `python3 -S` (`common/forge.rs:232`).
  The harness debounce is 1 ms, so no daemon timer causes this time.
  The source shows the fetch counts. The cost of about 0.4–0.6 s per fetch is an inference from the durations, not a measurement.
- **Shared fixtures**: `both_seats_script` (ranks 9, 13, 17) and `kept_request_running` (ranks 8, 19).
- **A fixed 5 s herdr probe** in ranks 7 and 10. These are the only tests where one fixed wait is most of the time.
- **Backoff and tick waits** in ranks 15 and 16.
- `QUIET` (100 ms, `common/mod.rs:76`) occurs at most 3 times in a test, so it adds at most 0.3 s.

The causes come from a read of the source, and no test was traced.
Each function line above was checked, and each `sleep` in these tests is a `QUIET` listen.
The `fetch_again`, `stub.fetches()`, `QUIET`, `Timeouts` and loop counts were taken from the function bodies.
The stub `ssh` call counts of ranks 6 and 12 are counted from call sites, not measured.

## Slowest desktop tests

The desktop suite has 140 files and 1480 tests.
Single tests are short: the highest median is 0.88 s, and the longest single observation is 0.93 s (rank 3).
The ranking uses the median of six runs.
`forge-hosts.test.tsx` has two tests with the same name. The CSV keeps both and marks the second one `[duplicate name]`.
The test bodies sum to 143–146 s of the cumulative worker time.
Vitest reports the cumulative time as import 36–37 %, tests 34–35 %, environment 20 %, transform 4–6 % and setup 4 %.
So module import and environment setup cost more than the test bodies.

| Rank | File | Test | Min s | Median s | Max s |
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

Slowest files by the sum of their test bodies (min / median / max s over six runs):

| Rank | File | Seconds |
| ---: | --- | ---: |
| 1 | `src/features/sessions/console/session-console.test.tsx` | 5.60 / 5.89 / 6.10 |
| 2 | `src/features/agents/agents-page.test.tsx` | 5.47 / 5.57 / 5.79 |
| 3 | `src/features/goals/hosts.test.tsx` | 4.55 / 4.66 / 4.90 |
| 4 | `src/features/pull-requests/hosts.test.tsx` | 4.31 / 4.48 / 4.66 |
| 5 | `src/features/forge/forge-hosts.test.tsx` | 4.16 / 4.46 / 4.63 |
| 6 | `src/features/skills/skills-page.test.tsx` | 3.89 / 4.00 / 4.10 |
| 7 | `src/features/permissions/permissions-page.test.tsx` | 3.91 / 3.95 / 4.00 |
| 8 | `src/features/sessions/console/composer.test.tsx` | 3.50 / 3.57 / 3.65 |
| 9 | `src/features/permissions/ai-test-panel.test.tsx` | 3.46 / 3.49 / 3.60 |
| 10 | `src/features/system/daemon-logs-drawer.test.tsx` | 3.27 / 3.45 / 3.60 |

The full per-test lists are [`desktop-test-durations.csv`](linux-host/desktop-test-durations.csv) and [`dashboard-test-durations.csv`](linux-host/dashboard-test-durations.csv).
The dashboard suite has 9 files and 56 tests. Import takes 58–61 % of its time, and tests take 2–3 %.

## Failed and intermittent tests

Nextest ran 12 times on one unchanged revision: series a and series b.
Five runs exited 100. Each failed test passed in the other runs, so each one is intermittent.

| Test | Failed in | Output |
| --- | --- | --- |
| `relay::stop_kills_the_full_process_group` | 3 of 12 runs | `relay.rs:248`: `ConnectionReset` (os error 104) where the test expects EOF |
| `ai_permissions_decisions::a_stopped_model_warns_and_waits_for_the_console` | 1 of 12 runs | `ai_permissions_decisions.rs:1160`: no `WARN` line in the log snapshot |
| `ai_permissions_decisions::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval` | 1 of 12 runs | `ai_permissions_decisions.rs:867`: 3 model requests, expected 2 |

[`failures.md`](linux-host/failures.md) gives the full output.
No flaky marker, retry or skip was added.
The cause of each failure is not investigated here.
All desktop and dashboard runs passed: 1480 and 56 tests.

## Findings about the method

- The daemon build script (`crates/ariadne-daemon/build.rs`) names every tracked and untracked file in `rerun-if-changed`.
  A change to any untracked file in the worktree therefore recompiles `ariadne-daemon`.
  The first driver wrote its results into the worktree, and two warm binary builds took 8.4 s and 11.0 s.
  Those rows are in [`invalid-bins-untracked-evidence.jsonl`](linux-host/invalid-bins-untracked-evidence.jsonl) and do not count.
  The valid series writes its results under `target/`, which git ignores.
- Nextest writes `junit.xml` under the workspace `target/nextest/ci`, not under `CARGO_TARGET_DIR`.
  Series a lost its per-test data for this reason. Series b repeated test compilation and execution with the copy fixed.
  Series a times agree with series b: compilation 147.8–148.4 s cold and 1.02–1.07 s warm, execution 78.1–79.6 s.
- `cargo check` in `ui/src-tauri` writes `ui/src-tauri/gen/`, and a dependency writes `target/generated/bincode` in the worktree.
  Git ignores both paths.

## Gaps

- No tool was missing for the measured stages. `mold`, `clang` and `sccache` are absent, and no named command needs them.
- `shellcheck` and `actionlint` are absent, so the shell and workflow checks under [Report checks](README.md#report-checks) did not run.
  This change adds no shell script and no workflow. `git diff --check`, the link check and `test_run.py` passed.
- CI installs `libxdo-dev` and `libssl-dev` for Tauri. They are absent here, and `cargo check` passed without them.
- The load before each run comes mostly from the previous stage of this task, because the load average lags.
  The 1-minute load before a run was 3.8–28.5. The table in [`tables.md`](linux-host/tables.md) gives each value.
  `ps` showed no other process with a high CPU share, but the host is a container, and the load of its neighbours is not visible.
- Warm `npm ci` was not measured. The criteria do not name it.
- No run flushed the OS page cache, and the download caches were warm copies.
- The linker data shows link calls only. It does not split compile time from link time.

## Reproduction

1. Check out `226a47eb63c8ab763167b3fbedc89e52a0e66ab3` in a clean worktree.
2. Copy `~/.cargo/registry`, `~/.cargo/git` and `~/.npm` into `target/latency-host` as `cargo-home/` and `npm-cache/`.
3. Run `cargo fetch --locked` with that `CARGO_HOME`, at the root and in `ui/src-tauri`.
4. Put the linker wrapper at `target/latency-host/linkbin/cc`.
5. Run `python3 run.py snapshot <rep>` for reps 1–3.
6. Run `python3 run.py run <stage> cold <rep>`, then `warm <rep>`, for each stage and rep.
   Set `LATENCY_SERIES=b` for the test series.
7. Run `python3 summarize.py`.
8. Delete each private target when its pair is done.
