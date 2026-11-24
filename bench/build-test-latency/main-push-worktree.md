# Main-push worktree comparison

Source commit: `9ded33b51b62cc66a568439aa5943029f98c005d`. Date: 2026-11-24.

Both hooks passed their complete check set.

The previous hook took 317.97 seconds. The new hook took 267.44 seconds.

This single pair does not establish a stable speed improvement.
The host was shared, and external download caches remained available.

## Method

The measurement used this task worktree on macOS 27.0.1, build 26A434, arm64.
The machine had 16 logical CPUs and 64 GiB RAM.
Tools: Rust 1.99.0, nextest 0.9.146, Node 26.12.0, npm 11.20.0.
`CARGO_TARGET_DIR`, `RUSTC_WRAPPER`, and `CARGO_INCREMENTAL` were unset.

Both runs used the source commit above with no tracked changes.
A saved Cargo target snapshot supplied identical initial build artifacts for both runs.
The snapshot contained warm dependency artifacts and locally rebuilt store tests.
APFS copies used `cp -cR target /tmp/ftcz7z-comparison-snapshot`.
Between runs, the completed target moved aside.
A clone of that snapshot restored `target` at its original path.
Both runs started without `ui/node_modules`, `ui/dashboard/node_modules`, or `ui/src-tauri/target`.
The previous hook shared the restored target with its temporary checkout.
The new hook used that target directly.

Frozen executable copies kept both hooks stable during measurement.
Their `HOOK_DIR` pointed at this worktree.
The previous copy called its own frozen executable for its recursive `--full` invocation.
These path substitutions changed no check commands or their order.
The new executable retained the original repository root and safety checks.

Each executable received this stand-in push line on standard input:

```text
HEAD <source-commit> refs/heads/main <source-commit>
```

The command was `/tmp/ftcz7z-<before|after>-hook origin`.
No measurement pushed to a remote.
A Python runner used `time.monotonic()` and a 600-second limit for each complete hook.
It captured combined output under `/tmp/ftcz7z-<before|after>.log`.
Each command duration spans its printed start through the next command start.
The final duration includes hook cleanup or final safety checks.
Total time also includes hook startup.

## Results

Each row follows the unchanged execution order.
Durations use seconds.

| Directory | Command | Previous | New |
| --- | --- | ---: | ---: |
| `root` | `cargo machete .` | 0.09 | 0.12 |
| `root` | `cargo audit` | 2.05 | 2.38 |
| `root` | `cargo build --locked --workspace --bins` | 22.56 | 20.43 |
| `root` | `cargo nextest run --workspace --profile ci` | 105.93 | 95.70 |
| `root` | `cargo test --doc --workspace` | 1.67 | 1.80 |
| `root` | `cargo clippy --workspace --all-targets -- -D warnings` | 20.62 | 22.88 |
| `root` | `cargo fmt --all -- --check` | 0.88 | 0.94 |
| `root` | `scripts/check-unused-rust` | 9.97 | 11.14 |
| `root` | `scripts/check-migrations.sh` | 0.60 | 0.84 |
| `ui` | `npm ci` | 3.31 | 12.34 |
| `ui` | `npm run check:audit` | 2.16 | 1.99 |
| `ui` | `npm run lint` | 1.43 | 1.24 |
| `ui` | `npm run typecheck` | 8.10 | 5.82 |
| `ui` | `npm run check:unused` | 0.39 | 0.41 |
| `ui` | `npm test` | 64.62 | 60.03 |
| `ui/dashboard` | `npm ci` | 36.29 | 3.96 |
| `ui` | `npm run build` | 17.26 | 7.24 |
| `ui/dashboard` | `npm run check:audit` | 0.97 | 0.89 |
| `ui/dashboard` | `npm run lint` | 0.75 | 0.74 |
| `ui/dashboard` | `npm run typecheck` | 1.05 | 1.02 |
| `ui/dashboard` | `npm run check:unused` | 0.22 | 0.22 |
| `ui/dashboard` | `npm test` | 1.71 | 1.69 |
| `ui/src-tauri` | `cargo fmt --all -- --check` | 0.06 | 0.07 |
| `ui/src-tauri` | `cargo check` | 15.02 | 13.47 |
| All | Total | 317.97 | 267.44 |

Raw aggregates: [previous](main-push-before.json), [new](main-push-after.json).

## Excluded preliminary runs

Editing the active Bash hook disrupted the first baseline run.
That run supplied no comparison result.
A preliminary new-hook run stopped at stale migration test binaries.
Their compiled migration paths named a deleted temporary checkout.
That run took 131.49 seconds and failed at nextest.
It did not reach the remaining checks and cannot support a total-time comparison.
We rebuilt store artifacts without changing source or tests, then saved the shared comparison snapshot.
Both reported runs started from that snapshot.

## Boundary verification

`bash scripts/tests/githooks.sh main-push` asserts every full-check command and its order.
It also proves checks use the pushing worktree without another checkout.
`main-nested` proves remaining untracked nested repositories cause refusal before checks.
The hook names each directory and requires moving or removing it by hand.
It preserves nested repositories and never uses forced repository deletion.
This later safety addition does not change the measured check commands.
The cleanup and refusal cases appear in [the script conventions](../../scripts/AGENTS.md).
`full` proves manual full checks still accept tracked changes and retain untracked files.

One existing fixture fails at the unchanged source revision.
`release-please-config-commit` lacks `pull-request-title-pattern` in its synthetic configuration.
Its release configuration test therefore fails before the intended assertions.
This hook change does not alter that fixture.

## Existing worktrees with stale manifest paths

The first main push can fail after this hook change.
A clean index and the correct HEAD do not prevent this failure.
The new hook does not currently repair stale artifacts or print a specific repair command.

The two migration tests compile `env!("CARGO_MANIFEST_DIR")` into their binaries.
They use that absolute path at runtime to read the migration directory.
The previous hook built them in a temporary checkout, then deleted that checkout.
Their shared target directory retained the binaries.

Cargo 1.99.0 accepted those binaries as fresh after the source checkout changed.
The raw rustc `.d` file retained `# env-dep:CARGO_MANIFEST_DIR=<deleted-checkout>`.
Cargo's translated fingerprint dep-info omitted that variable and stored the source path relative to the package.
With matching files and old source mtimes, Cargo reported the target as fresh.
The binary still contained the deleted absolute path.
Runtime migration loading therefore failed with `No such file or directory`.

Cargo's [dep-info translation](https://doc.rust-lang.org/stable/nightly-rustc/src/cargo/compiler/fingerprint/dep_info.rs.html#314-368)
filters compiler-provided environment variables and makes package source paths relative.
This explains the observed combination of a fresh fingerprint and a stale compiled manifest path.

A dependency-free probe reproduced this behavior with the compiler wrapper disabled.
The user Cargo configuration selects sccache even when the `RUSTC_WRAPPER` environment variable is unset.
The wrapper-free reproduction shows that sccache is not required for this failure.

Reproduce the Cargo freshness decision without changing product source:

```sh
task_probe=$(mktemp -d)
mkdir -p "$task_probe/gone/src"
cat > "$task_probe/gone/Cargo.toml" <<'TOML'
[package]
name = "fresh_probe"
version = "0.1.0"
edition = "2024"
TOML
cat > "$task_probe/gone/src/main.rs" <<'RUST'
fn main() {
    let path = env!("CARGO_MANIFEST_DIR");
    println!("{path}");
    assert!(std::path::Path::new(path).exists());
}
RUST
CARGO_TARGET_DIR="$task_probe/target" RUSTC_WRAPPER= \
    cargo run --offline --manifest-path "$task_probe/gone/Cargo.toml" -vv
cp -pR "$task_probe/gone" "$task_probe/live"
mv "$task_probe/gone" "$task_probe/retained-source"
CARGO_TARGET_DIR="$task_probe/target" RUSTC_WRAPPER= \
    CARGO_LOG=cargo::compiler::fingerprint=trace \
    cargo run --offline --manifest-path "$task_probe/live/Cargo.toml" -vv
```

The first command passed. The second command exited 101.
Cargo printed `Fresh fresh_probe`, then ran the old binary.
That binary printed the previous manifest directory and failed its path assertion.
The fingerprint trace reported that all paths were up to date.
Logs: `/tmp/ftcz7z-no-wrapper-probe.log` and `/tmp/ftcz7z-fresh-probe.log`.

A second stand-in repository exercised the current hook from revision `cefd2fc0`.
It built one path-reading test in a temporary Git worktree, then removed that worktree.
The pushing repository retained the shared target directory and the committed source and lockfile.
The hook used real Cargo build and nextest commands. Other Cargo checks used stand-ins.
Its first simulated main push exited 1 at `cargo nextest run --workspace --profile ci`.
The test failed because its compiled manifest directory no longer existed.
No remote push occurred, and the temporary Git worktree was removed.
Logs: `/tmp/ftcz7z-stale-current-hook.log` and `/tmp/ftcz7z-stale-probe-summary.log`.
The earlier full Ariadne measurement also observed this failure in both migration tests.

Two repair options await the orchestrator's decision:

1. Detect stale compiled manifest paths and refuse with a specific manual repair command.
   For the observed store failures, `cargo clean -p ariadne-store` followed by another push repairs the artifacts.
   The earlier local repair rebuilt the package and passed all 229 store tests.
   This option preserves the check command sequence and requires one explicit user action.
2. Detect stale compiled manifest paths and rebuild affected package artifacts once before checks.
   This option adds a conditional repair step and requires decisions about detection, package scope, and concurrent target access.

Neither option requires removing a full target directory.
No repair option has been selected or added to the hook.
