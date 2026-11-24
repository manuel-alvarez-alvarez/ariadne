# Host malvarez: the cost of the fake forge CLI

These results come from host malvarez only.
Do not pool them with the GitHub runner data or the macOS data in [the main report](README.md).

The reader decides whether the fake `gh` of the daemon tests is a cost worth its fix.
This page measures where the time of the five slowest forge tests goes, and the effect of a cheaper fake.

## Result

- Before the change, the fake `gh` processes are the main cost of the five tests: 54–60 % of the wall time of each test, run alone.
- One fake call took about 24 ms: `sh` started `python3 -S`, which took 8 ms to start and 10 ms more to import `json` and `threading`.
- The new fake is a POSIX `sh` program. One call takes 1.4–1.5 ms, and the fake calls take 6–7 % of the wall time of each test.
- In the full run, the median of each of the five tests goes from 7.1–8.6 s to 1.6–2.1 s.
- The forge test modules go from 41.4–42.0 s to 25.1–25.5 s (−39 %).
- The full nextest run goes from 79.7–81.1 s to 64.0–64.7 s (−20 %).
- Each `gh` call is still a real child process of the daemon. No file under `src/` changes.

## Host and revision

The base revision is `de90ce670e5b62a9f3f769aaa013464384b821c4` (main).
The measured change is commit `0e5ccc950d8088e6570fbeb09a4c0fac62adb59f`. It changes only files under `crates/ariadne-daemon/tests/it/`.
The host is as in [the Linux report](linux-host.md#host-and-revision): AMD Ryzen 7 5700U, 16 threads, 32 GiB, an LXC container.
Rust is 1.100.0, cargo-nextest 0.9.152, Python 3.13.3, and `/bin/sh` is dash.
The rows of the result ran on 2026-11-24 from 01:56 to 02:14 UTC.

## Method

### Where the time goes

[`fake-forge/trace.py`](fake-forge/trace.py) runs one test of the `it` binary under `strace -f --seccomp-bpf -e trace=execve`.
It takes each child process from its `execve` to its exit.

- The fake `gh` time is the union of the intervals of the processes started through the `gh` link.
- The fixture time is the time from the test start to the first `gh` call: the harness, the store copy and the git fixtures.
- The rest is the daemon work, the ACP stub sessions and the test code.

Each test ran alone, so the figures show the cost of each part without contention.
`strace` makes each `execve` a little slower, so the absolute figures are a little high.

```sh
cargo nextest run --workspace --profile ci --no-run
python3 bench/build-test-latency/fake-forge/trace.py <it binary> <test> /tmp/out.strace
```

The output is in [`attribution-before.txt`](fake-forge/attribution-before.txt) (base) and [`attribution-after.txt`](fake-forge/attribution-after.txt) (`0e5ccc95`).

### Suite times

[`fake-forge/run.py`](fake-forge/run.py) runs two commands once each, and appends one row per run to [`results.jsonl`](fake-forge/results.jsonl):

```sh
cargo nextest run --workspace --profile ci --status-level fail --final-status-level fail -E '<forge modules>'
cargo nextest run --workspace --profile ci --status-level fail --final-status-level fail
```

The forge modules are the 18 modules of the `it` binary that call `stub_forge_cli`. `run.py` names them in `MODULES`:

```text
binary(it) & (test(/^attention::/) | test(/^forge_host_limit::/) | test(/^forge_integration::/) | test(/^forge_rate_limit::/) | test(/^hosts::/) | test(/^issues::/) | test(/^kept_requests::/) | test(/^pull_request_babysit::/) | test(/^pull_request_detail_skip::/) | test(/^pull_request_keep_retry::/) | test(/^pull_request_lists::/) | test(/^pull_request_reviews::/) | test(/^pull_requests::/) | test(/^stub_lifetime::/) | test(/^tunnel::/) | test(/^webhooks::/) | test(/^workflow_pull_request::/) | test(/^workflow_steps::/))
```

Before each timed run, `run.py` builds the tests until a build compiles nothing.
The daemon build script reruns when any file of the worktree or the git index changes, `results.jsonl` too.
Each row keeps:

- `sha` (HEAD), `base`, and `files`: the SHA-256 of each test file that differs between them, on disk, at `base` and at HEAD;
- `tree`: `base` or `head`, the revision that all of the files on disk match;
- the exit code, the wall time, the runner time from the nextest summary, and `compiled`, true if the timed run compiled;
- the JUnit time of the five tests, the failed tests, and the idle CPU share for 2 s before the run.

[`fake-forge/round.sh`](fake-forge/round.sh) runs one interleaved round on the committed branch:

1. It writes the base revision's version of each test file that differs from HEAD.
2. It runs `run.py` for the base files.
3. It checks out the HEAD files again.
4. It runs `run.py` for the change.

```sh
bench/build-test-latency/fake-forge/round.sh de90ce670e5b62a9f3f769aaa013464384b821c4 1   # then 2 and 3
```

### Rows that the result does not use

`results.jsonl` also keeps earlier rows. No figure on this page uses them:

- `sequential-1`, `sequential-2` and `interleaved-draft` have no file hashes.
- Many of them, and all `interleaved-unwarmed` rows, include a rebuild of 16–20 s in their wall time, for the build-script reason above.
- The rebuild ran in some runs only. The earlier draft of this page read the difference as a drift of the host speed, and that was wrong.
- The `after` rows of `sequential-1` ran a draft of the fake with the reader defect in [Failures](#failures).
- The draft rows (`sequential-2` `after` and `interleaved-draft` `after`) ran the fake without the input watch.

## Where the time goes

Each test ran alone. The figures are in seconds, except where the column names another unit.

| Rank | Test | Wall before | `gh` calls | `gh` before | Share before | Wall after | `gh` after | Share after |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | `pull_request_babysit::asking_keeps_another_authors_request_until_the_merge` | 2.817 | 96 | 1.522 | 54 % | 2.158 | 0.142 | 7 % |
| 2 | `pull_request_babysit::asking_keeps_a_request_of_mine_by_hand_until_the_merge` | 2.724 | 94 | 1.488 | 55 % | 2.170 | 0.140 | 6 % |
| 3 | `pull_request_babysit::asking_keeps_a_dependabot_request_until_the_merge` | 2.793 | 97 | 1.520 | 54 % | 2.135 | 0.134 | 6 % |
| 4 | `pull_request_reviews::a_review_round_needs_an_ask_and_runs_on_the_current_head` | 2.660 | 89–91 | 1.595 | 60 % | 2.044 | 0.140 | 7 % |
| 5 | `pull_request_reviews::a_reply_in_a_review_thread_starts_one_round` | 2.073 | 66–68 | 1.121 | 54 % | 1.495 | 0.105 | 7 % |

The fixture time is 0.40–0.47 s before and after the change.
Before the change, the rest is 0.55–0.83 s. After the change, it is 0.99–1.58 s.
The wall time falls by less than the `gh` time, because the daemon and the test now wait on other things that the slow calls hid: the ACP sessions, the scheduler and the polls.
Alone, each test takes 2.1–2.8 s before the change. In the full run, it takes 7–9 s.
Thus, in the full run, most of the time of each test is contention for the CPU, and the CPU cost of each call counts more than its wall time.

## Suite times

These are interleaved rounds, three each. No timed run compiled. The wall time includes the nextest start; the runner time is the nextest summary.

| Stage | Before wall, s | Before runner, s | After wall, s | After runner, s | Change, wall |
| --- | --- | --- | --- | --- | ---: |
| Forge modules | 41.399 / 41.583 / 41.977 | 40.384 / 40.606 / 41.012 | 25.452 / 25.145 / 25.520 | 24.522 / 24.183 / 24.496 | −39 % |
| Full nextest run | 79.733 / 81.105 / 80.920 | 78.637 / 79.988 / 79.859 | 64.043 / 64.470 / 64.728 | 62.918 / 63.341 / 63.619 | −20 % |

The base has 380 forge tests and 2660 in all. The change adds one test, so it has 381 and 2661.
The before exits are 0, 0 and 100 for the modules, and 100, 0 and 0 for the full run. Each exit 100 is a failure of the base, listed in [Failures](#failures). All the after runs exited 0.

The JUnit time of the five tests in the three full runs, min / median / max:

| Rank | Test | Before, s | After, s |
| ---: | --- | --- | --- |
| 1 | `asking_keeps_another_authors_request_until_the_merge` | 8.307 / 8.480 / 8.494 | 2.029 / 2.099 / 2.118 |
| 2 | `asking_keeps_a_request_of_mine_by_hand_until_the_merge` | 8.100 / 8.273 / 8.928 | 2.009 / 2.089 / 2.102 |
| 3 | `asking_keeps_a_dependabot_request_until_the_merge` | 7.849 / 8.634 / 9.358 | 1.980 / 2.003 / 2.230 |
| 4 | `a_review_round_needs_an_ask_and_runs_on_the_current_head` | 8.052 / 8.268 / 8.403 | 1.897 / 1.961 / 1.981 |
| 5 | `a_reply_in_a_review_thread_starts_one_round` | 6.945 / 7.103 / 7.217 | 1.545 / 1.570 / 1.675 |

## The change

`stub_forge_cli` in `crates/ariadne-daemon/tests/it/common/forge.rs` keeps its interface and its behaviour:

- The script answers: the test side compiles the JSON script into a `sh` function, `answer`, on each reprogram. Each body is one single-quoted word. The test side writes the file aside and renames it, so a call reads the old script or the new one, never a part of each.
- The call log and the finished log: each record goes into the next free slot file of its log. `set -C` creates the slot with `O_EXCL`, so two concurrent calls never share one. The fields end with a NUL, so an argument can hold any other byte.
- The fetch count reads the finished log, as before.
- The held calls: a held call checks its gate every 20 ms with `sleep 0.02`, as before.
- The exit when the test process dies: a held call checks its parent every 0.2 s. A read of `--input -` runs in the background beside a watch that does the same check, and the watch ends the read and the call when the parent has ended. The check is `kill -0` and `ps -o ppid=`, because `kill -0` alone still succeeds for an unreaped zombie and `$PPID` is fixed at start.

The program uses only `sh`, `cat`, `sleep` and `ps`, with one code path for Linux and macOS.
`/bin/sh` is dash on Ubuntu and bash in POSIX mode on macOS.
On this host, the forge tests also passed with `bash --posix` as the shell.
The change was not run on a macOS host.

## Failures

No flaky marker, retry or skip was added. Each failure is reported with its output.

### Exposed by the faster fake, fixed in the test

`pull_request_reviews::a_review_resolves_its_own_fixed_finding` failed 19 of 20 runs alone with the new fake, before its fix:

```text
panicked at crates/ariadne-daemon/tests/it/common/mod.rs:2286:9:
assertion `left == right` failed: {"error":{"code":"not_found","message":"pull_request_comment not found: rc-501"}}
  left: 404
 right: 200
```

The review post wakes a fetch. The script of the test did not list the posted finding, so the fetch removed its row.
With the old fake, the fetch was slow enough that the resolve came first.
The test now adds the finding to the forge list after the post, and waits until a fetch reads it back into its GitHub thread.
After the fix, it passed 30 of 30 runs alone, and 40 of 40 runs under 16 busy loops.
Its name and its assertions did not change.

### Defects of drafts of the new fake, fixed

`pull_request_detail_skip::a_fetch_after_a_failed_detail_read_runs_the_detail_read_again` failed once in the `sequential-1` `after` series:

```text
panicked at crates/ariadne-daemon/tests/it/common/mod.rs:2418:9:
timed out waiting for the second detail read
```

`Cli::run` kills a call that a failed sibling abandons.
A call killed between the slot claim and the write left an empty slot, and the reader of the draft stopped at that slot.
The reader now skips a slot that is not whole.
After the fix, the test passed 40 of 40 runs under 16 busy loops.

The review of the first commit found that a read of `--input -` had no parent check.
A call whose input pipe stayed open could outlive its test.
The new test `stub_lifetime::a_forge_stub_reading_its_input_exits_when_its_parent_is_killed` holds that pipe open and kills the parent.
Without the fix, it failed:

```text
panicked at crates/ariadne-daemon/tests/it/common/mod.rs:2418:9:
timed out waiting for the orphaned stub to exit
```

With the input watch, it passes.

### On the base revision, intermittent

Each of these failed once with the base test files, and passed in the other runs of this page.

`pull_request_babysit::a_keeper_whose_agent_dies_the_moment_it_starts_gives_up_after_the_budget`, `sequential-2` `before` full run 1:

```text
panicked at crates/ariadne-daemon/tests/it/pull_request_babysit.rs:1321:5:
assertion `left == right` failed: a keeper given up on after the budget is not spawned again
  left: 4
 right: 3
```

`ariadne-console tui::subagents::tests::a_session_without_a_child_has_no_bar_and_tab_switches_nothing`, `interleaved` `before` full run 1. The rendered elapsed time crossed a second between the two renders:

```text
panicked at crates/ariadne-console/src/tui/subagents.rs:453:9:
assertion `left == right` failed
  left: "● parent words\n author · claude:opus · running   ⠋ thinking 105238m 13s\n…
 right: "● parent words\n author · claude:opus · running   ⠋ thinking 105238m 12s\n…
```

`tunnel::a_proxy_in_the_environment_never_carries_the_post_to_the_listener` and `tunnel::a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once`, `interleaved` `before` modules run 3:

```text
panicked at crates/ariadne-daemon/tests/it/tunnel.rs:1268:5:
assertion `left == right` failed
  left: 200
 right: 201
panicked at crates/ariadne-daemon/tests/it/tunnel.rs:565:5:
assertion `left == right` failed
  left: ["v1/health", "DuDN3JyGEGHsnG4sN9dyDZ"]
 right: ["DuDN3JyGEGHsnG4sN9dyDZ"]
```

The second output shows a `v1/health` request on the stand-in server of the test. Thus another process probably reached a port that the test had picked as free.

Under 16 busy loops, `hosts::updates_wait_for_idle_and_retry_without_replacing_installed_binaries` ran past the 10 s `SLOW` mark once (10.959 s), and it passed. It uses the fake `ssh`, which this task does not change.
The three intermittent tests of [the Linux failures](linux-host/failures.md) did not fail in the runs of this page.

## Report checks

Run `git diff --check` for whitespace, and check each relative Markdown target on this page.
