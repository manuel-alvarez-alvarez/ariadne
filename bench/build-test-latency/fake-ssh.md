# Shell fake ssh on macOS: the cost of the fake ssh

These results come from one Mac only.
Do not pool them with the GitHub runner data, the Linux data or the other macOS data in [the main report](README.md).

The reader decides whether the fake `ssh` of the daemon tests is a cost worth its fix.
This page measures where the time of the five slowest `hosts` tests goes, and the effect of a cheaper fake.
Its method follows the fake forge report of the fake `gh`, `fake-forge.md` in this directory on `main`.

## Result

- Before the change, the fake `ssh` processes are the largest part of the five tests: 52–76 % of the wall time of each test, run alone.
- Each fake call started `python3 -S` behind an `sh` launcher. That start alone takes 27–32 ms on this Mac, against 4 ms for `sh`.
- The new fake is a POSIX `sh` program. Only a forward (`-L` with `forward`) still starts `python3`, for the TCP relay that `sh` cannot do.
- Alone, each of the five tests is 0.17–0.69 s faster (−12 % to −29 %). The mean `ssh` call falls by 25–28 ms.
- In the parallel runs, no gain shows. The modules take 20.0–21.0 s before and 19.4–20.5 s after. The full run takes 77.1–83.4 s before and 78.8–83.4 s after.
- Thus, on this Mac, contention for the CPU sets the time of the parallel runs, not the start of the fake.
- Each `ssh` call is still a real child process of the daemon. No file under `src/` changes.

## Host and revision

- Base revision: `593dd44750e87a62b128f6b995b57aba07472d0d`, the merge base with `main`.
- Measured change: commit `4bf6b1a4`, which changes only files under `crates/ariadne-daemon/tests/it/`.
- macOS 27.0.1, build 26A434, Apple M4 Max, 16 logical CPUs, 64 GiB RAM.
- rustc 1.99.0 (Homebrew), cargo-nextest 0.9.146, Python 3.15.0. `/bin/sh` is bash 3.2 in POSIX mode.
- The rows ran on 2026-11-24 from 15:17 to 15:29 local time.
- Other work ran on this Mac during the runs, the real daemon and other agent sessions among it. The load average before each run is in each row.

## Method

### Where the time goes

SIP blocks `dtrace` on this Mac, and macOS has no `strace`.
Thus [`fake-ssh/trace.py`](fake-ssh/trace.py) traces with [`fake-ssh/wrap.c`](fake-ssh/wrap.c), a small C program:

1. It runs the test once with `TMPDIR` set to a private directory, so the harness writes its shared launchers (`shared_script`) there.
2. It puts the wrapper in the place of each launcher, and of `git` and `tmux` on the `PATH`.
3. It runs the test again. The wrapper keeps its pid: it forks a watcher, then execs the real program. The watcher waits for the exit with `kqueue` and logs the start and the exit.

- The fake `ssh` time is the union of the intervals of the `ssh` runs. It includes the remote commands that the fake runs, such as the setup scripts and the stub login shell.
- The fixture time runs from the test start to the first `ssh` call.
- The other children are the ACP agent stub, the stub service tools, `git` and `tmux`, outside the `ssh` time.
- The rest is the daemon work and the test code.

Each test ran alone, three times. The wrapper adds about one fork for each traced child.

```sh
cargo nextest run --workspace --profile ci --no-run
python3 bench/build-test-latency/fake-ssh/trace.py <it binary> hosts::<test> <new work directory>
```

The output is in [`attribution-before.txt`](fake-ssh/attribution-before.txt) (base) and [`attribution-after.txt`](fake-ssh/attribution-after.txt) (`4bf6b1a4`).

### Suite times

[`fake-ssh/run.py`](fake-ssh/run.py) runs two commands once each:

```sh
cargo nextest run --workspace --profile ci --no-fail-fast --status-level fail --final-status-level fail -E '<ssh modules>'
cargo nextest run --workspace --profile ci --no-fail-fast --status-level fail --final-status-level fail
```

The ssh modules are the five modules of the `it` binary that call `stub_ssh`:

```text
binary(it) & (test(/^hosts::/) | test(/^host_health::/) | test(/^host_reconnect::/) | test(/^ssh_forward::/) | test(/^stub_lifetime::/))
```

- Before each timed run, `run.py` builds the tests until a build compiles nothing. No timed run compiled.
- During the runs, `run.py` writes its rows, logs and JUnit files to `/tmp/fake-ssh-latency/`, outside the worktree. The daemon build script reruns when a file of the worktree changes.
- After the runs, the rows were copied to [`results.jsonl`](fake-ssh/results.jsonl).
- Each row keeps the HEAD, the base, and the SHA-256 of each test file that differs between them, on disk, at the base and at HEAD. `tree` names the revision that all of the files on disk match.

[`fake-ssh/round.sh`](fake-ssh/round.sh) runs one interleaved round on the committed branch:

1. It writes the base revision's version of each test file that differs from HEAD.
2. It runs `run.py` for the base files.
3. It checks out the HEAD files again.
4. It runs `run.py` for the change.

```sh
bench/build-test-latency/fake-ssh/round.sh 593dd44750e87a62b128f6b995b57aba07472d0d 1   # then 2 and 3
```

### Rows that the result does not use

The first three rounds timed `f069e395`, before the watch beside each setup command. The review sent that commit back, so this page times `4bf6b1a4`. The rows of `f069e395` are in `results.jsonl` at commit `12840c10`. They also showed no gain: modules 20.4–20.5 s before and 20.0–20.6 s after, full run 76.1–83.3 s before and 80.5–82.2 s after.

## Where the time goes

Each test ran alone, three times. The figures are the medians, in seconds, except where the column names another unit.

| Rank | Test | Wall before | `ssh` calls | `ssh` before | Mean call before | Share before | Wall after | `ssh` after | Mean call after | Share after |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | `selected_repositories_clone_only_committed_files_and_retry_registration` | 2.367 | 22 | 1.243 | 56.5 ms | 52–53 % | 1.731 | 0.642 | 29.2 ms | 35–38 % |
| 2 | `an_update_upgrades_a_runtime_that_setup_installed_and_leaves_one_the_user_installed` | 1.781 | 43–45 | 1.323 | 83.8 ms | 73–76 % | 1.550 | 0.953 | 57.3 ms | 60–64 % |
| 3 | `selected_credentials_cross_ssh_without_entering_progress_and_retry_skips_the_copy` | 2.405 | 26 | 1.616 | 62.2 ms | 67–68 % | 1.715 | 0.937 | 36.0 ms | 53–55 % |
| 4 | `setup_reads_the_interactive_shell_path_first_and_falls_back_in_order` | 1.431 | 39–40 | 0.996 | 80.2 ms | 66–71 % | 1.262 | 0.709 | 52.0 ms | 56–57 % |
| 5 | `repository_setup_copies_this_machines_host_key_to_the_worker` | 1.966 | 20 | 1.174 | 60.8 ms | 59–60 % | 1.452 | 0.680 | 35.9 ms | 46–47 % |

The rank is the JUnit time of a `hosts` module run at the base. All five tests are in the `hosts` module.

- The fixture time is 0.22–0.51 s before and 0.22–0.55 s after the change.
- The other children take 0.00–0.04 s outside the `ssh` time.
- The rest is 0.14–0.64 s before and 0.27–0.64 s after.
- Tests 2 and 4 run their cases at the same time, so their `ssh` calls overlap.
- The `ssh` time that stays after the change is mostly the setup scripts that the fake runs through the stub login shell.

## Suite times

These are interleaved rounds, three each. No timed run compiled.
The wall time includes the nextest start; the runner time is the nextest summary.

| Stage | Before wall, s | Before runner, s | After wall, s | After runner, s | Change of the mean wall |
| --- | --- | --- | --- | --- | ---: |
| ssh modules | 19.966 / 20.191 / 20.992 | 19.493 / 19.715 / 20.516 | 19.408 / 20.219 / 20.473 | 18.933 / 19.744 / 19.999 | −1.7 % |
| Full nextest run | 77.114 / 83.430 / 82.273 | 76.604 / 82.915 / 81.758 | 78.848 / 83.422 / 79.591 | 78.317 / 82.911 / 79.079 | −0.4 % |

The ranges overlap in both stages. Three runs per side are few, and the host load changed between the runs (load average 5.5–34.3 before a run).
The base has 125 module tests and 2692 in all. The change adds three tests, so it has 128 and 2695.
In round 3, the build before the `after` modules run still compiled after three tries, and `run.py` stopped with `the test build does not settle`. The `after` half of round 3 then ran again with `run.py after 3 <base>`, with the same files. No row of the stopped try exists.
Every module run exited 100, for the base and for the change, by the failure in [Failures](#failures). Every full run exited 0.

The JUnit time of the five tests in the three full runs, min / median / max:

| Rank | Test | Before, s | After, s |
| ---: | --- | --- | --- |
| 1 | `selected_repositories_clone_only_committed_files_and_retry_registration` | 8.388 / 8.654 / 9.307 | 8.617 / 8.802 / 9.603 |
| 2 | `an_update_upgrades_a_runtime_that_setup_installed_and_leaves_one_the_user_installed` | 6.867 / 7.130 / 8.287 | 6.947 / 7.818 / 8.586 |
| 3 | `selected_credentials_cross_ssh_without_entering_progress_and_retry_skips_the_copy` | 5.079 / 5.179 / 5.188 | 4.859 / 5.516 / 5.689 |
| 4 | `setup_reads_the_interactive_shell_path_first_and_falls_back_in_order` | 6.254 / 6.968 / 7.743 | 6.670 / 6.989 / 7.249 |
| 5 | `repository_setup_copies_this_machines_host_key_to_the_worker` | 5.580 / 5.712 / 5.804 | 5.631 / 6.039 / 6.724 |

Alone, each test takes 1.3–2.4 s. In the full run, it takes 5–9 s.
Most of the time of each test in a parallel run is thus a wait for the CPU, and the saved start of `python3` is small against it.

## The change

`stub_ssh` in `crates/ariadne-daemon/tests/it/common/ssh.rs` keeps its interface and its behaviour:

- The script: the test side compiles the JSON script into `sh` variables, one single-quoted word each, with the defaults of the Python stub. The stub sources them on each call. A raw probe `PATH` may hold a NUL, so the stub copies it from a file.
- The order of the answers is that of the Python stub: `-G`, a `setup` command, the home call, the forward, the end.
- The run log: each run takes the next free slot file of its log. `set -C` creates the slot with `O_EXCL`, so two runs at the same time never share one. The fields end with a NUL, as in the fake `gh`.
- The forward: a run that reaches the forward execs `ssh-relay.py`, the relay part of the Python stub. It keeps connections at the same time, `connect_stderr`, `until` and `hold_reaping`, and writes the same slot records.
- The exit when the test process dies: a hung home call checks its parent every 0.2 s with `kill -0`, as the Python stub did. Each setup command runs in the background beside a watch that does the same check and ends the run, and the run waits for the command. As in the Python stub, the command goes on. A read of the `runtime_disconnect` FIFO has a watch of its own that ends the read.
- Each `sleep` writes to `/dev/null`, so it holds no pipe of the daemon open.
- Three new tests prove the parent check: `stub_lifetime::an_ssh_stub_whose_home_call_hangs_exits_when_its_parent_is_killed`, `stub_lifetime::an_ssh_stub_waiting_for_its_disconnect_exits_when_its_parent_is_killed` and `stub_lifetime::an_ssh_stub_whose_setup_command_blocks_exits_when_its_parent_is_killed`. Without the check, each one failed with `timed out waiting for the orphaned stub to exit`.

The stub uses only `sh`, `cat`, `sleep` and `kill`, with one code path for Linux and macOS. The forward still uses `python3`, which the suite already requires.

Two parts differ from the Python stub, on purpose:

- The Python stub split every setup command with `shlex` to look for the words `repo` and `ls` or `add`. The `sh` stub first looks for ` repo ls` or ` repo add ` in the command, and only then splits its words with `eval`. Thus it never runs the words of another command. Every `repo` command of the daemon holds one of these strings.
- A setup command that a signal kills makes the `sh` stub exit with 128 plus the signal. The Python stub exited with 256 minus the signal.
- A setup command runs as a background command of a shell without job control, so it ignores `SIGINT` and `SIGQUIT`. Under the Python stub, it did not. The daemon ends a setup run with `SIGKILL`, and no test sends these signals.

The change was not run on a Linux host. The Linux check runs on another host before the landing.

## Failures

No flaky marker, retry or skip was added. Each failure is reported with its output.

### Seen at the base and with the change

`host_health::a_direct_host_is_ready_while_it_answers_and_down_while_it_does_not` failed in every module run of this page, three at the base and three with the change. It also failed in each earlier module run of this task, at the base and with the change:

```text
panicked at crates/ariadne-daemon/tests/it/host_health.rs:204:5:
assertion `left == right` failed
  left: (Down, Pending)
 right: (Ready, InSync)
```

It passed in all six full runs, and in three runs alone at the base and three runs alone with the change.
It uses no fake `ssh`. Its failure depends on the set of tests that run beside it.

`host_health::a_direct_host_that_does_not_answer_is_down_and_ready_once_it_answers` ran past the 10 s `SLOW` mark in some module runs, at the base (18.4 s) and with the change (19.0–19.4 s). It passed each time, and it uses no fake `ssh`.

### Defects of drafts of the new fake, fixed

- The review of `12840c10` found that a setup command that blocks had no parent check: the run waited in `/bin/sh -c "$command"` alone. The runtime timeout test blocks such a command on a FIFO (`hosts.rs:4484`), and a killed test process leaves that FIFO without a writer. `stub_lifetime::an_ssh_stub_whose_setup_command_blocks_exits_when_its_parent_is_killed` failed on that stub with `timed out waiting for the orphaned stub to exit`, and it passes with the watch.
- The first draft matched the `runtime_disconnect` case with the pattern `?*:*"bounded provide"*` on `$u_runtime_disconnect:$command`. `?*` also matches a colon in the command, so every runtime install read an empty FIFO path. Thirteen `hosts` tests failed, most with `ended after 720s`. The stub now tests the FIFO path apart.
- The first draft put the raw probe `PATH` in a variable. `hosts::setup_rejects_control_characters_in_remote_path_before_transfer` failed with `sh cannot hold a NUL: "/tools\0/bin"`. The stub now copies that `PATH` from a file. For a NUL in a value of the setup environment, it exits 1 where Python failed to build that environment.

`hosts::an_update_upgrades_a_runtime_that_setup_installed_and_leaves_one_the_user_installed` timed out once in GitHub CI at `825a970f`. It did not time out or hang in the runs of this page.

## Report checks

Run `git diff --check` for whitespace, and check each relative Markdown target on this page.
