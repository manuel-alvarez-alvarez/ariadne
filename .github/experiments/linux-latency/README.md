# Isolated Linux latency experiment

This manual experiment implements the approved [design](../../../bench/build-test-latency/linux-ci-proposal.md).
It has not been dispatched. Landing and dispatch are separate operator actions.
Do not run project benchmark workloads locally or dispatch without separate approval.
Ordinary CI, hooks, test isolation, worker counts, retries and shared caches remain unchanged.

## Entry points and boundaries

The [workflow](../../workflows/linux-latency-experiment.yml) accepts only `experiment_id`,
validated as `[a-z0-9-]{1,32}` before interpolated input reaches any command.
All shell use passes the input through a quoted environment variable.
Nine matrix entries run serially: three independent repetitions each of Rust,
Tauri and both frontend packages together. Their timeout limits are 50, 30 and
25 minutes respectively: 315 runner minutes maximum, with a 171-minute planning
estimate. There is no automatic redispatch, replacement observation or retry.

[run.py](run.py) checks out no sources and accepts no arbitrary production commands.
The workflow separately checks out its implementation at `github.sha` and the
measured source at `4f71da2465954107f113d639b5d7bf666c8446a9`. Full measured history
and a freshly fetched `origin/main` support the migration guard; its resolved SHA
is evidence and a comparison boundary. HEAD is checked before setup and before
every phase. The command chains are the exact table in the design.
Rust verbose output is enabled through Cargo's environment setting, with the
same stable [linker wrapper](linker.sh) and instrumentation in all phases.

The first shell step starts a monotonic job budget before either checkout or
setup. Each foreground command gets at most 600 seconds, shortened by the
measurement deadline five minutes before the job timeout. Failure, timeout,
cancellation or the evidence limit stops the chain and marks all later commands
invalid. Process-group termination includes descendants even when the leader
has already exited. The foreground resource collector cannot outlive its runner.
The last five minutes allow one minute for private-server cleanup, three for
artifact upload and the remaining reporting overhead. GitHub hard termination
or setup exceeding the job limit can still prevent final evidence; the workflow
cannot guarantee upload after runner loss.

The public, unauthenticated API guard paginates all workflow runs, including
completed runs, to reject earlier same-ID runs whose completion overlaps the
current dispatch creation time. It fails closed for missing workflow visibility,
private repositories, rate limits and network/API errors. Workflow concurrency
also serializes same-ID races. GitHub itself may replace an older pending run
when another run joins a concurrency group; no application workload runs for
that replaced dispatch. Separate IDs never affect ordinary CI concurrency.

## Caches and evidence

Every job allocates a new, exact key including experiment, run, attempt, source,
actual image version, lane and repetition. Cargo, target, sccache and npm paths
are private siblings beneath `RUNNER_TEMP`; no fallback or cache-service keys
exist. Tool installation occurs outside them. Their manifests contain pinned
tools and lock hashes but no downloaded application dependencies.
The measured environment uses an allowlist and an empty private sccache config,
removing inherited remote backends and cache tokens. sccache has a private Unix
socket and is stopped both in the runner's `finally` and an `always()` cleanup
step. Runner teardown removes all caches, targets and checkouts, including after
hard cancellation. Nothing deletes or rewrites existing shared caches.

Cold uses empty application caches and targets, with no node_modules or
TypeScript build state. Warm retains all preceding state; `npm ci` still removes
and recreates node_modules. Rust replay retains downloads and sccache but uses
an empty replay target. It measures compiler-cache reuse independently of Cargo
artifact reuse. Tool setup and upload overhead are outside command timings;
setup elapsed time is in the identity manifest. Network downloads remain
identified in raw logs and bounded download excerpts. Cargo commands can mix
downloads, compilation and linking: no exclusive download/compilation split is
inferred from their total wall time.

Evidence includes exact argv/cwd, UTC timestamps, monotonic wall time, cumulative
child user/system CPU, raw logs, resource samples, Cargo timing HTML, per-command
sccache JSON before/after, test-runner summaries, JUnit counts/XML, and TypeScript
state inventories. State removed by npm scripts is visible by comparing those
inventories. Raw logs retain every test summary even when the structured excerpt
is capped at 100 lines of 1,024 characters. These are explicitly bounded excerpts.
Sampling records CPU/load, memory, filesystem use, device I/O and Linux
process-group CPU ticks/RSS/I/O every second. CPU in a persistent sccache server
is outside cumulative waited-child CPU and the command's foreground group;
these measurements do not establish exclusive compiler CPU or wall time.

Each [linker helper](linker.py) record includes command, PID, output, unchanged
arguments, monotonic start/end, GNU time elapsed/user/system and exit status.
An initial `started` record survives an interrupted link. Cargo crate-type logs
refine final-binary and procedural-macro attribution; build-script, test/other
and ambiguous shared-library links remain labeled. Driver sums can overlap and
must never be subtracted from command wall time. This measures `/usr/bin/cc`
driver costs, including overhead, rather than pure `ld` internals.

Artifacts retain evidence only for 14 days, never caches, targets, dependency
trees, tokens or a full environment. The live combined evidence limit accounts
for logs, links and samples, stops a command with `evidence_limit`/125 and reserves
64 MiB for structured final evidence. Any incomplete primary log is therefore
an explicitly invalid observation. Optional timing HTML omissions are recorded;
primary evidence is never silently removed to make an artifact fit. Upload is
bounded to two GiB per job; bootstrap metadata fits inside the structured reserve.
The final job summary records upload success/failure, including failures that
cannot be included in their own artifact. GitHub's upload action reports artifact
transfer details in the job log; upload duration is not a measured command.

After approved dispatch, collect all nine artifacts and run:

```sh
python3 .github/experiments/linux-latency/run.py report /path/to/artifacts/*/result.json
```

The report groups only identical experiment, measured/workflow SHA, lane, image,
tools, lockfiles, apt manifest, moving main SHA, runner version, kernel, CPU
model/count and RAM. Dynamic load, free memory and setup duration never split a
cohort. It reports failed/invalid rows and min/median/max per command plus paired
warm/cold ratio distributions for complete valid pairs. Duplicate repetition
numbers are refused. Fewer than three matching complete repetitions explicitly
reports `insufficient_matching_repetitions`; failed or changed-image runs are
never silently replaced. Historical audit data is never an input cohort.

## Checks

From the repository root, all checks are bounded fixtures or static checks:

```sh
python3 .github/experiments/linux-latency/test_run.py
shellcheck .github/experiments/linux-latency/linker.sh
actionlint -ignore 'label "ubuntu-26.04" is unknown' .github/workflows/linux-latency-experiment.yml
git diff --check
```

The local actionlint release has an older hosted-label catalog. Ignore only that
label warning; expression, matrix, action-input and shell diagnostics still fail.
Tests use tiny Python/sleep children and temporary paths, never Cargo/npm check
chains. They cover pair/replay state, private cache isolation, Tauri's unset
wrapper, fail-fast invalidation, timeout process groups, cancellation, deadline,
live evidence limits, optional omissions, exact chain boundaries, JUnit/linker
records, API pagination/failure and image/manifest cohort grouping.

## Preflight

Read-only checks on 2026-11-23 returned HTTP 200 at all of these exact endpoints:

| Pin | Endpoint |
| --- | --- |
| Rust/Cargo 1.100.0 | [release manifest](https://static.rust-lang.org/dist/channel-rust-1.100.0.toml) |
| nextest 0.9.146 | [release metadata](https://api.github.com/repos/nextest-rs/nextest/releases/tags/cargo-nextest-0.9.146) |
| sccache 0.16.0 | [release metadata](https://api.github.com/repos/mozilla/sccache/releases/tags/v0.16.0) |
| machete 0.9.2 | [crate metadata](https://crates.io/api/v1/crates/cargo-machete/0.9.2), [Linux assets](https://api.github.com/repos/bnjbvr/cargo-machete/releases/tags/v0.9.2) |
| audit 0.22.0 | [crate metadata](https://crates.io/api/v1/crates/cargo-audit/0.22.0), [Linux assets](https://api.github.com/repos/rustsec/rustsec/releases/tags/cargo-audit%2Fv0.22.0) |
| Node 26.12.0 | [release checksums](https://nodejs.org/dist/v26.12.0/SHASUMS256.txt) |
| npm 11.20.0 | [registry metadata](https://registry.npmjs.org/npm/11.20.0) |

The exact read-only availability operation was `urllib.request.urlopen` with a
25-second timeout and the `ariadne-latency-preflight` User-Agent, recording
`response.status`. For example, reproduce one endpoint without installing tools:

```sh
python3 - <<'PY'
import urllib.request
url = 'https://static.rust-lang.org/dist/channel-rust-1.100.0.toml'
request = urllib.request.Request(url, headers={'User-Agent': 'ariadne-latency-preflight'})
with urllib.request.urlopen(request, timeout=25) as response:
    print(response.status)
PY
```

GitHub `GET /repos/{owner}/{repo}/git/ref/tags/{tag}` returned 200 and resolved:

| Action tag | Immutable SHA |
| --- | --- |
| actions/checkout v7 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| actions/setup-node v7 | `949feb2413d6458794dcd2491c4babbbce0c15c1` |
| actions/upload-artifact v6 | `b7c566a772e6b6bfb58ed0dc250532a479d7789f` |
| taiki-e/install-action v2 | `fe3acb53a98b62c772db35de75ce0e4595e7ca8c` |

The immutable `action.yml` sources were fetched and inspected read-only at
`https://raw.githubusercontent.com/{owner}/{repo}/{SHA}/action.yml` (all 200),
as was install-action's `main.sh`. Checkout disables credential persistence;
setup-node explicitly disables its default automatic package-manager cache;
install-action keeps checksum verification enabled and disables fallback
installers; upload uses the normal artifact service. Actions run on Node 24
independently of the pinned measured Node binary. Their SHAs are also in every
full identity manifest. Only `contents: read` is requested; no secrets, OIDC,
cache tokens or repository write permissions are exported.

Availability is not installation proof. Remaining live preflight gaps are actual
`ubuntu-26.04` assignment and image/apt contents, runner binary layout/version,
GNU time/GCC/binutils/tmux versions, installer checksum/download behavior,
current advisories, real test/JUnit/Cargo/sccache interoperability, public API
visibility/rate budget, artifact service behavior, default-branch workflow
availability, operator permissions and approved runner-minute budget. The runner
requires an exact Runner.Listener version from `/home/runner/runners/*/bin/` or
`/home/runner/actions-runner/bin/`; an unknown layout fails preflight rather than
inventing a version. Record actual `ImageVersion` per lane/repetition; never
require a historical image. No hosted run, project timing, cache-service access,
shared-cache mutation, dependency change or infrastructure action occurred here.
