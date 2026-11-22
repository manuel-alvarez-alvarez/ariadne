# Baseline CI evidence

Run: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987645207

Revision: `0561a6ab4a5864186724fab5bcf783030359298b`.

One observed run; no controlled rerun. Times use API timestamps, rounded to seconds.

## UI

Job: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987645207/job/137101618699

| Step | Seconds | Result |
| --- | ---: | --- |
| Set up job | 1 | success |
| Run actions/checkout@v7 | 2 | success |
| Run actions/setup-node@v7 | 6 | success |
| Install dependencies | 4 | success |
| Audit dependencies | 1 | success |
| Lint | 0 | success |
| Typecheck | 7 | success |
| Check unused code | 0 | success |
| Test | 85 | success |
| Report test results | 1 | success |
| Post Run actions/setup-node@v7 | 0 | success |
| Post Run actions/checkout@v7 | 0 | success |
| Complete job | 0 | success |

## Rust (macos-latest)

Job: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987645207/job/137101618834

| Step | Seconds | Result |
| --- | ---: | --- |
| Set up job | 3 | success |
| Run actions/checkout@v7 | 6 | success |
| Install the Rust toolchain | 2 | success |
| Install sccache | 0 | success |
| Export the cache variables for sccache | 0 | success |
| Install tmux for relay tests | 16 | success |
| Install cargo-nextest | 1 | success |
| Install cargo-machete | 1 | success |
| Install cargo-audit | 1 | success |
| Cache cargo artifacts | 21 | success |
| Check for unused dependencies | 1 | success |
| Audit dependencies | 3 | success |
| Build | 88 | success |
| Test | 471 | success |
| Doctests | 10 | success |
| Clippy | 63 | success |
| Unused pub items | 18 | success |
| Migrations guard | 0 | success |
| Report test results | 1 | success |
| Show sccache statistics | 0 | success |
| Post Cache cargo artifacts | 0 | success |
| Post Run actions/checkout@v7 | 1 | success |
| Complete job | 2 | success |

## Tauri shell (macos-latest)

Job: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987645207/job/137101618860

| Step | Seconds | Result |
| --- | ---: | --- |
| Set up job | 2 | success |
| Run actions/checkout@v7 | 3 | success |
| Install the Rust toolchain | 1 | success |
| Cache cargo artifacts | 5 | success |
| Install Tauri's Linux dependencies | 0 | skipped |
| Check | 6 | success |
| Post Cache cargo artifacts | 0 | success |
| Post Run actions/checkout@v7 | 1 | success |
| Complete job | 0 | success |

## Rustfmt

Job: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987645207/job/137101618883

| Step | Seconds | Result |
| --- | ---: | --- |
| Set up job | 0 | success |
| Run actions/checkout@v7 | 3 | success |
| Install the Rust toolchain | 1 | success |
| Check formatting | 5 | success |
| Check formatting (Tauri shell) | 0 | success |
| Post Run actions/checkout@v7 | 0 | success |
| Complete job | 0 | success |

## Rust (ubuntu-latest)

Job: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987645207/job/137101618888

| Step | Seconds | Result |
| --- | ---: | --- |
| Set up job | 2 | success |
| Run actions/checkout@v7 | 4 | success |
| Install the Rust toolchain | 2 | success |
| Install sccache | 1 | success |
| Export the cache variables for sccache | 0 | success |
| Install tmux for relay tests | 6 | success |
| Install cargo-nextest | 1 | success |
| Install cargo-machete | 0 | success |
| Install cargo-audit | 0 | success |
| Cache cargo artifacts | 11 | success |
| Check for unused dependencies | 0 | success |
| Audit dependencies | 3 | success |
| Build | 45 | success |
| Test | 213 | success |
| Doctests | 3 | success |
| Clippy | 32 | success |
| Unused pub items | 13 | success |
| Migrations guard | 0 | success |
| Report test results | 0 | success |
| Show sccache statistics | 0 | success |
| Post Cache cargo artifacts | 0 | success |
| Post Run actions/checkout@v7 | 0 | success |
| Complete job | 0 | success |

## Tauri shell (ubuntu-latest)

Job: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987645207/job/137101618968

| Step | Seconds | Result |
| --- | ---: | --- |
| Set up job | 3 | success |
| Run actions/checkout@v7 | 5 | success |
| Install the Rust toolchain | 5 | success |
| Cache cargo artifacts | 8 | success |
| Install Tauri's Linux dependencies | 45 | success |
| Check | 10 | success |
| Post Cache cargo artifacts | 0 | success |
| Post Run actions/checkout@v7 | 1 | success |
| Complete job | 0 | success |

## Dashboard

Job: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987645207/job/137101618970

| Step | Seconds | Result |
| --- | ---: | --- |
| Set up job | 1 | success |
| Run actions/checkout@v7 | 2 | success |
| Run actions/setup-node@v7 | 4 | success |
| Install dependencies | 7 | success |
| Audit dependencies | 1 | success |
| Lint | 0 | success |
| Typecheck | 2 | success |
| Check unused code | 0 | success |
| Test | 3 | success |
| Report test results | 0 | success |
| Post Run actions/setup-node@v7 | 0 | success |
| Post Run actions/checkout@v7 | 0 | success |
| Complete job | 0 | success |
