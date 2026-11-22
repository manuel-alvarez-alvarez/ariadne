# Adjacent-revision CI observations

Run: https://github.com/manuel-alvarez-alvarez/ariadne/actions/runs/44987726140

Revision: `0c20584b9d5fb04b891e46374d7da1cd091b4d89`.

These are contextual observations, not baseline repetitions.
The release revision changes package versions, manifests, and lockfiles.
Do not pool its timings with the baseline for min, median, or max.

| Job | Build | Test | Clippy | Dependency installation |
| --- | ---: | ---: | ---: | ---: |
| Tauri shell (macos-latest) | — | — | — | — |
| Rust (ubuntu-latest) | 51.0 | 270.0 | 39.0 | — |
| Rustfmt | — | — | — | — |
| Dashboard | — | 4.0 | — | 10.0 |
| Rust (macos-latest) | 58.0 | 311.0 | 30.0 | — |
| UI | — | 112.0 | — | 5.0 |
| Tauri shell (ubuntu-latest) | — | — | — | — |

All numbers use API timestamps in seconds; each cell has one observation.
Retrieve these steps with `gh run view 44987726140 --json jobs,headSha,url`.
