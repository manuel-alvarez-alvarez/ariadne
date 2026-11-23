---
id: isolated-linux-latency
status: current
updated: 2026-11-23
areas: [bench, workflows]
commits: []
tests:
  - .github/experiments/linux-latency/test_run.py
  - .github/workflows/linux-latency-experiment.yml
---

# Isolated Linux latency measurements

## Scope

In: the approved manual hosted-Linux experiment, bounded orchestration, private
cache conditions, evidence collection and comparison of three repetitions.
Out: dispatch authorization, production optimization, ordinary CI changes,
shared or remote cache experiments, historical performance conclusions.
Ordinary CI cache policy belongs to spec 016, rule 19.

## Behavior

1. Only `workflow_dispatch` accepts a validated experiment ID. Nine serial
   matrix jobs cover three lanes and three repetitions. Rust/Tauri/frontend
   limits are 50/30/25 minutes, totalling 315 minutes. Only `contents: read`
   permissions and immutable action SHAs are used. A public unauthenticated
   paginated API guard rejects overlapping earlier same-ID runs, including
   completed runs that overlapped the dispatch creation time, and fails closed.
2. Every observation measures source `4f71da2465954107f113d639b5d7bf666c8446a9`,
   checked before setup and each phase. Workflow SHA is separate. Tools are
   pinned by the approved design. Record the actual hosted `ubuntu26` image
   version for every lane/repetition; no historical image build is required.
3. Private exact local keys include experiment/run/attempt/source/image/lane/
   repetition. Cold application caches and artifacts are empty. Warm retains
   state. Rust replay retains downloads/compiler cache and uses an empty target.
   Tauri has no compiler wrapper. No shared cache service, inherited remote
   sccache backend, cache token or application dependency prefetch is used.
4. Chains preserve all approved checks, assertions, worker limits and retry
   policies. Each command has a 600-second process-group deadline, shortened
   by a five-minute evidence reserve measured from job start including setup.
   Failure, timeout, cancellation or evidence exhaustion invalidates later
   commands. Collectors cannot outlive the runner; private servers stop in
   finally and an always cleanup step, with runner teardown as hard-stop fallback.
5. Evidence records exact commands, statuses, monotonic/CPU timing, resource
   samples, lock/tool/apt identities, logs, Cargo HTML, linker-driver records,
   sccache statistics and JUnit/test summaries. Driver arguments are unchanged;
   overlapping driver sums are never treated as exclusive command wall time.
   Evidence is bounded live with structured reserve, optional omissions are
   explicit, and incomplete output invalidates the observation. Artifacts retain
   at most two GiB per job for 14 days; upload outcome is in the job summary.
6. Reports compare only matching source/workflow/lane/image/tools/locks/apt/main/
   hardware/runner conditions. Duplicate repetitions fail. Complete valid pairs
   supply min/median/max command durations and paired warm/cold ratios. Fewer
   than three matching complete repetitions explicitly reports insufficiency;
   failures or image changes are never pooled or silently replaced.

## Acceptance criteria

- Trigger, nine serial jobs, budgets, pinned actions, permissions, no shared cache
  steps and always upload are proved by `test_workflow_is_dispatch_only_serial_and_bounded` in
  `test_run.py`, with `actionlint` validating workflow syntax; exact ordered chains are
  checked by `test_exact_chains_preserve_check_boundaries` in `test_run.py`.
- Source-phase checks, cold/warm/replay state and isolation are proved by
  `test_phase_source_mismatch_invalidates_later_observations`,
  `test_cold_warm_replay_retain_only_the_approved_state`,
  `test_private_keys_and_environment_disable_inherited_remote_cache` and
  `test_tauri_keeps_the_compiler_wrapper_unset_and_has_no_replay`.
- Failures, process-group timeouts, reserve and cancellation are proved by
  `test_failure_preserves_status_and_invalidates_downstream`,
  `test_timeout_kills_the_whole_process_group`,
  `test_deadline_prevents_new_commands` and
  `test_sigterm_preserves_partial_evidence_and_kills_children`.
- Evidence bounds and attribution are proved by
  `test_live_primary_cap_stops_a_noisy_command_with_visible_failure`,
  `test_evidence_limit_records_omissions_and_never_truncates_primary`,
  `test_linker_driver_preserves_arguments_and_records_command_attribution` and
  `test_junit_counts_and_runner_summaries_survive_evidence_collection`.
- API pagination, queued overlaps and fail-closed behavior are proved by
  `test_duplicate_guard_reads_every_page_and_catches_a_completed_overlap` and
  `test_duplicate_guard_fails_closed_without_api_access`.
- Cohort comparability and insufficient matching repetitions are proved by
  `test_images_and_package_manifests_never_pool`,
  `test_three_matching_images_report_distributions_and_ratios` and
  `test_duplicate_repetitions_are_refused_instead_of_replacing_evidence`.

## Known gap

Fixtures and static validation prove orchestration, not hosted timings or
installed-tool interoperability. Hosted preflight and dispatch require separate
approval; the [runner documentation](../.github/experiments/linux-latency/README.md#preflight)
names the unchecked live conditions. GitHub may replace pending runs under its
own concurrency semantics; replaced dispatches execute no application workload.

## Sources

[Approved amended design](../bench/build-test-latency/linux-ci-proposal.md),
[manual workflow](../.github/workflows/linux-latency-experiment.yml),
[runner](../.github/experiments/linux-latency/run.py),
[linker helper](../.github/experiments/linux-latency/linker.py),
[fixture tests](../.github/experiments/linux-latency/test_run.py).
