# Sequential follow-up measurements

Source baseline: `0561a6ab4a5864186724fab5bcf783030359298b`.
Report revision before measurements: `cb3a7efdcad2ca10d9a7ccf00b6d12acb0629556`.

See [method and cache conditions](reproduction.md) before comparing these numbers.
Each row records one observation. CPU numbers sum child-process user and system seconds.

## Individual observations

| Run | Wall s | CPU user s | CPU system s | Exit | Link calls | Link sum s | Link maximum s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| bins-1-cold | 33.602 | 63.272 | 13.025 | 0 | 80 | 4.95 | 0.28 |
| bins-1-warm | 0.207 | 0.097 | 0.106 | 0 | 0 | 0 | 0 |
| bins-2-cold | 33.704 | 64.048 | 13.517 | 0 | 80 | 5.05 | 0.29 |
| bins-2-warm | 0.208 | 0.097 | 0.107 | 0 | 0 | 0 | 0 |
| tests-build-1-cold | 49.655 | 132.756 | 20.772 | 0 | 93 | 6.51 | 0.32 |
| tests-build-1-warm | 0.487 | 0.376 | 0.27 | 0 | 0 | 0 | 0 |
| tests-build-2-cold | 49.856 | 134.225 | 20.999 | 0 | 93 | 6.95 | 0.3 |
| tests-build-2-warm | 0.484 | 0.376 | 0.268 | 0 | 0 | 0 | 0 |
| clippy-1-cold | 32.404 | 67.676 | 13.138 | 0 | 78 | 4.65 | 0.11 |
| clippy-1-warm | 3.792 | 4.951 | 0.744 | 0 | 0 | 0 | 0 |
| clippy-2-cold | 32.196 | 67.554 | 13.143 | 0 | 78 | 4.59 | 0.1 |
| clippy-2-warm | 0.257 | 0.133 | 0.117 | 0 | 0 | 0 | 0 |
| tauri-1-cold | 15.149 | 13.737 | 6.211 | 0 | 58 | 3.26 | 0.09 |
| tauri-1-warm | 0.14 | 0.066 | 0.071 | 0 | 0 | 0 | 0 |
| tauri-2-cold | 15.338 | 13.888 | 6.371 | 0 | 58 | 3.46 | 0.1 |
| tauri-2-warm | 0.141 | 0.067 | 0.07 | 0 | 0 | 0 | 0 |
| doctests-1 | 23.245 | 27.891 | 1.997 | 0 | 0 | 0 | 0 |
| machete-1 | 0.111 | 0.673 | 0.417 | 0 | 0 | 0 | 0 |
| audit-1 | 2.512 | 0.374 | 0.829 | 1 | 0 | 0 | 0 |
| unused-1 | 9.044 | 8.781 | 0.081 | 0 | 0 | 0 | 0 |
| migrations-1 | 0.531 | 0.153 | 0.18 | 0 | 0 | 0 | 0 |
| fmt-1 | 0.877 | 0.822 | 0.033 | 0 | 0 | 0 | 0 |
| shell-1 | 1.24 | 1.128 | 0.046 | 0 | 0 | 0 | 0 |
| doctests-2 | 1.534 | 1.26 | 0.262 | 0 | 0 | 0 | 0 |
| machete-2 | 0.082 | 0.559 | 0.231 | 0 | 0 | 0 | 0 |
| audit-2 | 1.86 | 0.348 | 0.76 | 1 | 0 | 0 | 0 |
| unused-2 | 8.884 | 8.797 | 0.081 | 0 | 0 | 0 | 0 |
| migrations-2 | 0.322 | 0.143 | 0.16 | 0 | 0 | 0 | 0 |
| fmt-2 | 0.856 | 0.82 | 0.03 | 0 | 0 | 0 | 0 |
| shell-2 | 1.159 | 1.116 | 0.03 | 0 | 0 | 0 | 0 |
| workspace-execution-1 | 64.66 | 239.447 | 262.808 | 0 | 3 | 0.86 | 0.32 |
| workspace-execution-2 | 61.706 | 244.771 | 263.273 | 0 | 3 | 0.75 | 0.27 |
| tauri-fmt-1 | 0.23 | 0.034 | 0.036 | 0 | 0 | 0 | 0 |
| tauri-fmt-2 | 0.049 | 0.034 | 0.009 | 0 | 0 | 0 | 0 |
| desktop-1-first-install | 2.337 | 2.171 | 3.802 | 0 | — | — | — |
| desktop-1-first-audit | 0.825 | 0.447 | 0.099 | 0 | — | — | — |
| desktop-1-first-lint | 0.958 | 0.972 | 0.29 | 0 | — | — | — |
| desktop-1-first-types | 4.04 | 6.587 | 0.263 | 0 | — | — | — |
| desktop-1-first-unused | 0.306 | 0.272 | 0.048 | 0 | — | — | — |
| desktop-1-first-test | 35.55 | 141.779 | 22.71 | 0 | — | — | — |
| desktop-1-warm-audit | 0.837 | 0.449 | 0.1 | 0 | — | — | — |
| desktop-1-warm-lint | 0.312 | 0.928 | 0.162 | 0 | — | — | — |
| desktop-1-warm-types | 3.841 | 6.43 | 0.252 | 0 | — | — | — |
| desktop-1-warm-unused | 0.305 | 0.274 | 0.045 | 0 | — | — | — |
| desktop-1-warm-test | 34.319 | 140.476 | 22.997 | 0 | — | — | — |
| dashboard-1-first-install | 4.019 | 3.566 | 7.298 | 0 | — | — | — |
| dashboard-1-first-audit | 0.885 | 0.53 | 0.115 | 0 | — | — | — |
| dashboard-1-first-lint | 0.856 | 0.248 | 0.151 | 0 | — | — | — |
| dashboard-1-first-types | 1.128 | 1.837 | 0.179 | 0 | — | — | — |
| dashboard-1-first-unused | 0.217 | 0.184 | 0.035 | 0 | — | — | — |
| dashboard-1-first-test | 2.063 | 4.316 | 1.025 | 0 | — | — | — |
| dashboard-1-warm-audit | 0.879 | 0.53 | 0.108 | 0 | — | — | — |
| dashboard-1-warm-lint | 0.23 | 0.215 | 0.051 | 0 | — | — | — |
| dashboard-1-warm-types | 1.014 | 1.804 | 0.158 | 0 | — | — | — |
| dashboard-1-warm-unused | 0.226 | 0.189 | 0.04 | 0 | — | — | — |
| dashboard-1-warm-test | 1.846 | 4.143 | 0.885 | 0 | — | — | — |
| frontend-build-1-first | 11.862 | — | — | 0 | — | — | — |
| frontend-build-1-warm | 11.602 | — | — | 0 | — | — | — |
| desktop-2-first-install | 2.401 | 2.155 | 3.813 | 0 | — | — | — |
| desktop-2-first-audit | 0.835 | 0.446 | 0.093 | 0 | — | — | — |
| desktop-2-first-lint | 0.967 | 0.967 | 0.286 | 0 | — | — | — |
| desktop-2-first-types | 3.985 | 6.529 | 0.258 | 0 | — | — | — |
| desktop-2-first-unused | 0.306 | 0.274 | 0.044 | 0 | — | — | — |
| desktop-2-first-test | 35.581 | 141.41 | 22.752 | 0 | — | — | — |
| desktop-2-warm-audit | 0.853 | 0.453 | 0.104 | 0 | — | — | — |
| desktop-2-warm-lint | 0.3 | 0.942 | 0.149 | 0 | — | — | — |
| desktop-2-warm-types | 3.883 | 6.476 | 0.243 | 0 | — | — | — |
| desktop-2-warm-unused | 0.311 | 0.277 | 0.048 | 0 | — | — | — |
| desktop-2-warm-test | 34.349 | 140.081 | 22.903 | 0 | — | — | — |
| dashboard-2-first-install | 4.092 | 3.549 | 7.392 | 0 | — | — | — |
| dashboard-2-first-audit | 0.885 | 0.53 | 0.115 | 0 | — | — | — |
| dashboard-2-first-lint | 0.75 | 0.249 | 0.166 | 0 | — | — | — |
| dashboard-2-first-types | 1.125 | 1.837 | 0.179 | 0 | — | — | — |
| dashboard-2-first-unused | 0.216 | 0.183 | 0.035 | 0 | — | — | — |
| dashboard-2-first-test | 2.11 | 4.321 | 1.123 | 0 | — | — | — |
| dashboard-2-warm-audit | 0.873 | 0.534 | 0.113 | 0 | — | — | — |
| dashboard-2-warm-lint | 0.248 | 0.22 | 0.053 | 0 | — | — | — |
| dashboard-2-warm-types | 1.005 | 1.771 | 0.159 | 0 | — | — | — |
| dashboard-2-warm-unused | 0.215 | 0.181 | 0.036 | 0 | — | — | — |
| dashboard-2-warm-test | 1.819 | 4.064 | 0.911 | 0 | — | — | — |
| frontend-build-2-first | 12.643 | — | — | 0 | — | — | — |
| frontend-build-2-warm | 12.046 | — | — | 0 | — | — | — |
| desktop-1-warm-install | 3.36 | 2.276 | 6.42 | 0 | — | — | — |
| dashboard-1-warm-install | 6.29 | 3.868 | 12.644 | 0 | — | — | — |
| desktop-2-warm-install | 3.29 | 2.304 | 6.458 | 0 | — | — | — |
| dashboard-2-warm-install | 6.363 | 3.846 | 12.658 | 0 | — | — | — |

## Repetition ranges

Each group contains the same command and corresponding cache condition.
Two observations provide a range; they do not provide a reliable distribution.

| Group | Count | Minimum s | Maximum s | Median s |
| --- | ---: | ---: | ---: | ---: |
| bins-N-cold | 2 | 33.602 | 33.704 | — |
| bins-N-warm | 2 | 0.207 | 0.208 | — |
| tests-build-N-cold | 2 | 49.655 | 49.856 | — |
| tests-build-N-warm | 2 | 0.484 | 0.487 | — |
| clippy-N-cold | 2 | 32.196 | 32.404 | — |
| clippy-N-warm | 2 | 0.257 | 3.792 | — |
| tauri-N-cold | 2 | 15.149 | 15.338 | — |
| tauri-N-warm | 2 | 0.140 | 0.141 | — |
| doctests-1 | 1 | 23.245 | 23.245 | — |
| machete-N | 2 | 0.082 | 0.111 | — |
| audit-1 | 1 | 2.512 | 2.512 | — |
| unused-N | 2 | 8.884 | 9.044 | — |
| migrations-N | 2 | 0.322 | 0.531 | — |
| fmt-N | 2 | 0.856 | 0.877 | — |
| shell-N | 2 | 1.159 | 1.240 | — |
| doctests-2 | 1 | 1.534 | 1.534 | — |
| audit-2 | 1 | 1.860 | 1.860 | — |
| workspace-execution-1 | 1 | 64.660 | 64.660 | — |
| workspace-execution-2 | 1 | 61.706 | 61.706 | — |
| tauri-fmt-N | 2 | 0.049 | 0.230 | — |
| desktop-N-first-install | 2 | 2.337 | 2.401 | — |
| desktop-N-first-audit | 2 | 0.825 | 0.835 | — |
| desktop-N-first-lint | 2 | 0.958 | 0.967 | — |
| desktop-N-first-types | 2 | 3.985 | 4.040 | — |
| desktop-N-first-unused | 2 | 0.306 | 0.306 | — |
| desktop-N-first-test | 2 | 35.550 | 35.581 | — |
| desktop-N-warm-audit | 2 | 0.837 | 0.853 | — |
| desktop-N-warm-lint | 2 | 0.300 | 0.312 | — |
| desktop-N-warm-types | 2 | 3.841 | 3.883 | — |
| desktop-N-warm-unused | 2 | 0.305 | 0.311 | — |
| desktop-N-warm-test | 2 | 34.319 | 34.349 | — |
| dashboard-N-first-install | 2 | 4.019 | 4.092 | — |
| dashboard-N-first-audit | 2 | 0.885 | 0.885 | — |
| dashboard-N-first-lint | 2 | 0.750 | 0.856 | — |
| dashboard-N-first-types | 2 | 1.125 | 1.128 | — |
| dashboard-N-first-unused | 2 | 0.216 | 0.217 | — |
| dashboard-N-first-test | 2 | 2.063 | 2.110 | — |
| dashboard-N-warm-audit | 2 | 0.873 | 0.879 | — |
| dashboard-N-warm-lint | 2 | 0.230 | 0.248 | — |
| dashboard-N-warm-types | 2 | 1.005 | 1.014 | — |
| dashboard-N-warm-unused | 2 | 0.215 | 0.226 | — |
| dashboard-N-warm-test | 2 | 1.819 | 1.846 | — |
| frontend-build-N-first | 2 | 11.862 | 12.643 | — |
| frontend-build-N-warm | 2 | 11.602 | 12.046 | — |
| desktop-N-warm-install | 2 | 3.290 | 3.360 | — |
| dashboard-N-warm-install | 2 | 6.290 | 6.363 | — |

## Exact commands and summaries

All commands use a 600-second timeout and separate external log files.
The table target names are relative to `target/latency-v2`.
No default target override applies when a row names no target.

### bins-1-cold

Directory: `.`.
Command: `cargo build --locked --workspace --bins --timings`.
Target: `bins-1`.
Condition: empty dedicated target; retained global downloads.

```text
      Timing report saved to /Users/malvarez/.ariadne/worktrees/xa86p35e/57b3g1py-eng/target/latency-v2/bins-1/cargo-timings/cargo-timing-20261122T160828986Z-79eff53b20f93c86.html
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.57s
```

### bins-1-warm

Directory: `.`.
Command: `cargo build --locked --workspace --bins --timings`.
Target: `bins-1`.
Condition: same target after cold command.

```text
      Timing report saved to /Users/malvarez/.ariadne/worktrees/xa86p35e/57b3g1py-eng/target/latency-v2/bins-1/cargo-timings/cargo-timing-20261122T160902590Z-79eff53b20f93c86.html
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.18s
```

### bins-2-cold

Directory: `.`.
Command: `cargo build --locked --workspace --bins --timings`.
Target: `bins-2`.
Condition: empty dedicated target; retained global downloads.

```text
      Timing report saved to /Users/malvarez/.ariadne/worktrees/xa86p35e/57b3g1py-eng/target/latency-v2/bins-2/cargo-timings/cargo-timing-20261122T160902796Z-79eff53b20f93c86.html
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.67s
```

### bins-2-warm

Directory: `.`.
Command: `cargo build --locked --workspace --bins --timings`.
Target: `bins-2`.
Condition: same target after cold command.

```text
      Timing report saved to /Users/malvarez/.ariadne/worktrees/xa86p35e/57b3g1py-eng/target/latency-v2/bins-2/cargo-timings/cargo-timing-20261122T160936502Z-79eff53b20f93c86.html
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.18s
```

### tests-build-1-cold

Directory: `.`.
Command: `cargo nextest run --workspace --no-run --cargo-profile dev`.
Target: `tests-build-1`.
Condition: empty dedicated target; retained global downloads.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 47.96s
```

### tests-build-1-warm

Directory: `.`.
Command: `cargo nextest run --workspace --no-run --cargo-profile dev`.
Target: `tests-build-1`.
Condition: same target after cold command.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s
```

### tests-build-2-cold

Directory: `.`.
Command: `cargo nextest run --workspace --no-run --cargo-profile dev`.
Target: `tests-build-2`.
Condition: empty dedicated target; retained global downloads.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 48.38s
```

### tests-build-2-warm

Directory: `.`.
Command: `cargo nextest run --workspace --no-run --cargo-profile dev`.
Target: `tests-build-2`.
Condition: same target after cold command.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s
```

### clippy-1-cold

Directory: `.`.
Command: `cargo clippy --workspace --all-targets -- -D warnings`.
Target: `clippy-1`.
Condition: empty target.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 32.32s
```

### clippy-1-warm

Directory: `.`.
Command: `cargo clippy --workspace --all-targets -- -D warnings`.
Target: `clippy-1`.
Condition: same target after cold clippy.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.71s
```

### clippy-2-cold

Directory: `.`.
Command: `cargo clippy --workspace --all-targets -- -D warnings`.
Target: `clippy-2`.
Condition: empty target.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 32.11s
```

### clippy-2-warm

Directory: `.`.
Command: `cargo clippy --workspace --all-targets -- -D warnings`.
Target: `clippy-2`.
Condition: same target after cold clippy.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.18s
```

### tauri-1-cold

Directory: `ui/src-tauri`.
Command: `cargo check`.
Target: `tauri-1`.
Condition: empty target.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.12s
```

### tauri-1-warm

Directory: `ui/src-tauri`.
Command: `cargo check`.
Target: `tauri-1`.
Condition: same target after cold check.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
```

### tauri-2-cold

Directory: `ui/src-tauri`.
Command: `cargo check`.
Target: `tauri-2`.
Condition: empty target.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.31s
```

### tauri-2-warm

Directory: `ui/src-tauri`.
Command: `cargo check`.
Target: `tauri-2`.
Condition: same target after cold check.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
```

### doctests-1

Directory: `.`.
Command: `cargo test --doc --workspace`.
Target: `bins-1`.
Condition: existing bin artifacts; first invocation.

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 21.78s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### machete-1

Directory: `.`.
Command: `cargo machete .`.
Target: `None`.
Condition: existing bin artifacts; first invocation.

```text
Analyzing dependencies of crates in ....
```

### audit-1

Directory: `.`.
Command: `cargo audit`.
Target: `None`.
Condition: existing bin artifacts; first invocation.

```text
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1560 security advisories (from /Users/malvarez/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (577 crate dependencies)
Crate:     url
Version:   2.5.8
Title:     Stack overflow in `Url::origin()` on nested `blob:` URLs
Date:      2026-11-20
ID:        RUSTSEC-2026-0592
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0592
Solution:  Upgrade to >=2.5.9

Crate:     chrono
Version:   0.4.45
Warning:   unmaintained
Warning:   unmaintained
Title:     `zvariant` is unmaintained
Date:      2026-10-16
ID:        RUSTSEC-2026-0370
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0370

Crate:     zvariant_derive
Version:   5.15.1
Warning:   unmaintained
Title:     `zvariant_derive` is unmaintained
Date:      2026-10-16
ID:        RUSTSEC-2026-0372
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0372

Crate:     async-executor
Version:   1.14.0
Warning:   yanked

error: 1 vulnerability found!
warning: 5 allowed warnings found
```

### unused-1

Directory: `.`.
Command: `scripts/check-unused-rust`.
Target: `None`.
Condition: existing bin artifacts; first invocation.

```text
Exit status 0; no matching summary output.
```

### migrations-1

Directory: `.`.
Command: `scripts/check-migrations.sh`.
Target: `None`.
Condition: existing bin artifacts; first invocation.

```text
Exit status 0; no matching summary output.
```

### fmt-1

Directory: `.`.
Command: `cargo fmt --all -- --check`.
Target: `None`.
Condition: existing bin artifacts; first invocation.

```text
Exit status 0; no matching summary output.
```

### shell-1

Directory: `.`.
Command: `bash -c shellcheck scripts/*.sh .githooks/*`.
Target: `None`.
Condition: existing bin artifacts; first invocation.

```text
Exit status 0; no matching summary output.
```

### doctests-2

Directory: `.`.
Command: `cargo test --doc --workspace`.
Target: `bins-1`.
Condition: repeat; caches retained.

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.17s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### machete-2

Directory: `.`.
Command: `cargo machete .`.
Target: `None`.
Condition: repeat; caches retained.

```text
Analyzing dependencies of crates in ....
```

### audit-2

Directory: `.`.
Command: `cargo audit`.
Target: `None`.
Condition: repeat; caches retained.

```text
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1560 security advisories (from /Users/malvarez/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (577 crate dependencies)
Crate:     url
Version:   2.5.8
Title:     Stack overflow in `Url::origin()` on nested `blob:` URLs
Date:      2026-11-20
ID:        RUSTSEC-2026-0592
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0592
Solution:  Upgrade to >=2.5.9

Crate:     chrono
Version:   0.4.45
Warning:   unmaintained
Warning:   unmaintained
Title:     `zvariant` is unmaintained
Date:      2026-10-16
ID:        RUSTSEC-2026-0370
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0370

Crate:     zvariant_derive
Version:   5.15.1
Warning:   unmaintained
Title:     `zvariant_derive` is unmaintained
Date:      2026-10-16
ID:        RUSTSEC-2026-0372
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0372

Crate:     async-executor
Version:   1.14.0
Warning:   yanked

error: 1 vulnerability found!
warning: 5 allowed warnings found
```

### unused-2

Directory: `.`.
Command: `scripts/check-unused-rust`.
Target: `None`.
Condition: repeat; caches retained.

```text
Exit status 0; no matching summary output.
```

### migrations-2

Directory: `.`.
Command: `scripts/check-migrations.sh`.
Target: `None`.
Condition: repeat; caches retained.

```text
Exit status 0; no matching summary output.
```

### fmt-2

Directory: `.`.
Command: `cargo fmt --all -- --check`.
Target: `None`.
Condition: repeat; caches retained.

```text
Exit status 0; no matching summary output.
```

### shell-2

Directory: `.`.
Command: `bash -c shellcheck scripts/*.sh .githooks/*`.
Target: `None`.
Condition: repeat; caches retained.

```text
Exit status 0; no matching summary output.
```

### workspace-execution-1

Directory: `.`.
Command: `cargo nextest run --workspace --cargo-profile dev --status-level fail --final-status-level slow`.
Target: `tests-build-1`.
Condition: compiled workspace tests; retained fixture templates after first execution.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.99s
    Starting 2589 tests across 13 binaries
     Summary [  56.379s] 2589 tests run: 2589 passed, 0 skipped
```

### workspace-execution-2

Directory: `.`.
Command: `cargo nextest run --workspace --cargo-profile dev --status-level fail --final-status-level slow`.
Target: `tests-build-1`.
Condition: compiled workspace tests; retained fixture templates after first execution.

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.54s
    Starting 2589 tests across 13 binaries
     Summary [  55.800s] 2589 tests run: 2589 passed, 0 skipped
```

### tauri-fmt-1

Directory: `ui/src-tauri`.
Command: `cargo fmt --all -- --check`.
Target: `None`.
Condition: format check; no compiler artifacts needed.

```text
Exit status 0; no matching summary output.
```

### tauri-fmt-2

Directory: `ui/src-tauri`.
Command: `cargo fmt --all -- --check`.
Target: `None`.
Condition: format check; no compiler artifacts needed.

```text
Exit status 0; no matching summary output.
```

### desktop-1-first-install

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm ci`.

```text
added 557 packages, and audited 564 packages in 2s
found 0 vulnerabilities
```

### desktop-1-first-audit

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run check:audit`.

```text
Exit status 0; no matching summary output.
```

### desktop-1-first-lint

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run lint`.

```text
Checked 429 files in 76ms. No fixes applied.
```

### desktop-1-first-types

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run typecheck`.

```text
Exit status 0; no matching summary output.
```

### desktop-1-first-unused

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run check:unused`.

```text
Exit status 0; no matching summary output.
```

### desktop-1-first-test

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm test -- --reporter=dot`.

```text
···stdout | src/features/goals/hosts.test.tsx > names the host in the title of a goal that failed to load there
stdout | src/features/goals/hosts.test.tsx > names the host in the title of a goal that failed to load there
···stdout | src/features/sessions/hosts.test.tsx > names the host in the titles of a session that failed to load there
stdout | src/features/sessions/hosts.test.tsx > names the host in the titles of a session that failed to load there
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of a sessions list that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of a sessions list that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of an activity tab that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of an activity tab that failed on a named host
stdout | src/features/tasks/hosts.test.tsx > retries a failed task on the host the panel is open on
stdout | src/features/tasks/hosts.test.tsx > retries a failed task on the host the panel is open on
······stdout | src/features/tasks/hosts.test.tsx > names the host in the title of a task that failed to load there
stdout | src/features/tasks/hosts.test.tsx > names the host in the title of a task that failed to load there
  error: null,
    error: null,
  error: null,
    error: null,
 Test Files  142 passed (142)
      Tests  1497 passed (1497)
   Duration  34.51s (tests 37%, import 35%, environment 20%, setup 4%, transform 4%)
```

### desktop-1-warm-audit

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run check:audit`.

```text
Exit status 0; no matching summary output.
```

### desktop-1-warm-lint

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run lint`.

```text
Checked 429 files in 66ms. No fixes applied.
```

### desktop-1-warm-types

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run typecheck`.

```text
Exit status 0; no matching summary output.
```

### desktop-1-warm-unused

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run check:unused`.

```text
Exit status 0; no matching summary output.
```

### desktop-1-warm-test

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm test -- --reporter=dot`.

```text
···········stdout | src/features/goals/hosts.test.tsx > names the host in the title of a goal that failed to load there
··stdout | src/features/goals/hosts.test.tsx > names the host in the title of a goal that failed to load there
·······stdout | src/features/sessions/hosts.test.tsx > names the host in the titles of a session that failed to load there
stdout | src/features/sessions/hosts.test.tsx > names the host in the titles of a session that failed to load there
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of a sessions list that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of a sessions list that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of an activity tab that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of an activity tab that failed on a named host
stdout | src/features/tasks/hosts.test.tsx > retries a failed task on the host the panel is open on
stdout | src/features/tasks/hosts.test.tsx > retries a failed task on the host the panel is open on
··stdout | src/features/tasks/hosts.test.tsx > names the host in the title of a task that failed to load there
····stdout | src/features/tasks/hosts.test.tsx > names the host in the title of a task that failed to load there
  error: null,
    error: null,
  error: null,
    error: null,
 Test Files  142 passed (142)
      Tests  1497 passed (1497)
   Duration  33.97s (tests 38%, import 36%, environment 20%, setup 4%, transform 2%)
```

### dashboard-1-first-install

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm ci`.

```text
added 381 packages, and audited 382 packages in 4s
18 moderate severity vulnerabilities
```

### dashboard-1-first-audit

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm run check:audit`.

```text
Exit status 0; no matching summary output.
```

### dashboard-1-first-lint

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm run lint`.

```text
Checked 27 files in 19ms. No fixes applied.
```

### dashboard-1-first-types

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm run typecheck`.

```text
Exit status 0; no matching summary output.
```

### dashboard-1-first-unused

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm run check:unused`.

```text
Exit status 0; no matching summary output.
```

### dashboard-1-first-test

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm test -- --reporter=dot`.

```text
 Test Files  9 passed (9)
      Tests  56 passed (56)
   Duration  1.32s (import 59%, environment 22%, setup 10%, tests 4%, transform 4%)
```

### dashboard-1-warm-audit

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm run check:audit`.

```text
Exit status 0; no matching summary output.
```

### dashboard-1-warm-lint

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm run lint`.

```text
Checked 27 files in 5ms. No fixes applied.
```

### dashboard-1-warm-types

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm run typecheck`.

```text
Exit status 0; no matching summary output.
```

### dashboard-1-warm-unused

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm run check:unused`.

```text
Exit status 0; no matching summary output.
```

### dashboard-1-warm-test

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm test -- --reporter=dot`.

```text
 Test Files  9 passed (9)
      Tests  56 passed (56)
   Duration  1.52s (import 59%, environment 23%, setup 11%, tests 5%, transform 2%, worker 1%)
```

### frontend-build-1-first

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run build`.

```text
Exit status 0; no matching summary output.
```

### frontend-build-1-warm

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm run build`.

```text
Exit status 0; no matching summary output.
```

### desktop-2-first-install

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm ci`.

```text
added 557 packages, and audited 564 packages in 2s
found 0 vulnerabilities
```

### desktop-2-first-audit

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run check:audit`.

```text
Exit status 0; no matching summary output.
```

### desktop-2-first-lint

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run lint`.

```text
Checked 429 files in 77ms. No fixes applied.
```

### desktop-2-first-types

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run typecheck`.

```text
Exit status 0; no matching summary output.
```

### desktop-2-first-unused

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run check:unused`.

```text
Exit status 0; no matching summary output.
```

### desktop-2-first-test

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm test -- --reporter=dot`.

```text
···stdout | src/features/goals/hosts.test.tsx > names the host in the title of a goal that failed to load there
stdout | src/features/goals/hosts.test.tsx > names the host in the title of a goal that failed to load there
···stdout | src/features/sessions/hosts.test.tsx > names the host in the titles of a session that failed to load there
stdout | src/features/sessions/hosts.test.tsx > names the host in the titles of a session that failed to load there
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of a sessions list that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of a sessions list that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of an activity tab that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of an activity tab that failed on a named host
stdout | src/features/tasks/hosts.test.tsx > retries a failed task on the host the panel is open on
stdout | src/features/tasks/hosts.test.tsx > retries a failed task on the host the panel is open on
··stdout | src/features/tasks/hosts.test.tsx > names the host in the title of a task that failed to load there
stdout | src/features/tasks/hosts.test.tsx > names the host in the title of a task that failed to load there
  error: null,
    error: null,
  error: null,
    error: null,
 Test Files  142 passed (142)
      Tests  1497 passed (1497)
   Duration  34.54s (tests 37%, import 35%, environment 20%, setup 4%, transform 4%)
```

### desktop-2-warm-audit

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run check:audit`.

```text
Exit status 0; no matching summary output.
```

### desktop-2-warm-lint

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run lint`.

```text
Checked 429 files in 62ms. No fixes applied.
```

### desktop-2-warm-types

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run typecheck`.

```text
Exit status 0; no matching summary output.
```

### desktop-2-warm-unused

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run check:unused`.

```text
Exit status 0; no matching summary output.
```

### desktop-2-warm-test

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm test -- --reporter=dot`.

```text
········stdout | src/features/goals/hosts.test.tsx > names the host in the title of a goal that failed to load there
stdout | src/features/goals/hosts.test.tsx > names the host in the title of a goal that failed to load there
·········stdout | src/features/sessions/hosts.test.tsx > names the host in the titles of a session that failed to load there
stdout | src/features/sessions/hosts.test.tsx > names the host in the titles of a session that failed to load there
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of a sessions list that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of a sessions list that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of an activity tab that failed on a named host
stdout | src/features/sessions/hosts.test.tsx > names the host in the title of an activity tab that failed on a named host
stdout | src/features/tasks/hosts.test.tsx > retries a failed task on the host the panel is open on
stdout | src/features/tasks/hosts.test.tsx > retries a failed task on the host the panel is open on
··stdout | src/features/tasks/hosts.test.tsx > names the host in the title of a task that failed to load there
stdout | src/features/tasks/hosts.test.tsx > names the host in the title of a task that failed to load there
  error: null,
    error: null,
  error: null,
    error: null,
 Test Files  142 passed (142)
      Tests  1497 passed (1497)
   Duration  33.99s (tests 38%, import 35%, environment 20%, setup 4%, transform 2%)
```

### dashboard-2-first-install

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm ci`.

```text
added 381 packages, and audited 382 packages in 4s
18 moderate severity vulnerabilities
```

### dashboard-2-first-audit

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm run check:audit`.

```text
Exit status 0; no matching summary output.
```

### dashboard-2-first-lint

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm run lint`.

```text
Checked 27 files in 19ms. No fixes applied.
```

### dashboard-2-first-types

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm run typecheck`.

```text
Exit status 0; no matching summary output.
```

### dashboard-2-first-unused

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm run check:unused`.

```text
Exit status 0; no matching summary output.
```

### dashboard-2-first-test

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm test -- --reporter=dot`.

```text
 Test Files  9 passed (9)
      Tests  56 passed (56)
   Duration  1.37s (import 60%, environment 22%, setup 10%, tests 4%, transform 4%)
```

### dashboard-2-warm-audit

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm run check:audit`.

```text
Exit status 0; no matching summary output.
```

### dashboard-2-warm-lint

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm run lint`.

```text
Checked 27 files in 5ms. No fixes applied.
```

### dashboard-2-warm-types

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm run typecheck`.

```text
Exit status 0; no matching summary output.
```

### dashboard-2-warm-unused

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm run check:unused`.

```text
Exit status 0; no matching summary output.
```

### dashboard-2-warm-test

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm test -- --reporter=dot`.

```text
 Test Files  9 passed (9)
      Tests  56 passed (56)
   Duration  1.50s (import 59%, environment 23%, setup 12%, tests 5%, transform 1%, worker 1%)
```

### frontend-build-2-first

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run build`.

```text
Exit status 0; no matching summary output.
```

### frontend-build-2-warm

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm run build`.

```text
Exit status 0; no matching summary output.
```

### desktop-1-warm-install

Directory: `target/latency-v2/frontend-corrected-1`.
Command: `npm ci`.

```text
added 557 packages, and audited 564 packages in 3s
found 0 vulnerabilities
```

### dashboard-1-warm-install

Directory: `target/latency-v2/frontend-corrected-1/dashboard`.
Command: `npm ci`.

```text
added 381 packages, and audited 382 packages in 6s
18 moderate severity vulnerabilities
```

### desktop-2-warm-install

Directory: `target/latency-v2/frontend-corrected-2`.
Command: `npm ci`.

```text
added 557 packages, and audited 564 packages in 3s
found 0 vulnerabilities
```

### dashboard-2-warm-install

Directory: `target/latency-v2/frontend-corrected-2/dashboard`.
Command: `npm ci`.

```text
added 381 packages, and audited 382 packages in 6s
18 moderate severity vulnerabilities
```

