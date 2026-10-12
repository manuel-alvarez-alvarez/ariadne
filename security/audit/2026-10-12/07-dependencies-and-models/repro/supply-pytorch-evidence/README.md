# Evidence for SUPPLY-007 (torch `weights_only` unpickler, CVE-2026-24747)

Exact command, platform, and output for the claim that a fresh install of
the pinned production Kev command resolves `torch==2.8.0`, and the exact
OSV.dev query used to check all 76 resolved packages for known advisories.

## Command

```
python3.13 -m venv venv
venv/bin/pip install --dry-run --report kev_dryrun_report.json \
  "kev[serve] @ git+https://github.com/jaredpalmer/kev@f1535963cea021439370c23127bc970b6788e730"
```

No index URL, extra index, or pip config override was set — this is pip's
default resolution against PyPI, plus `download.pytorch.org/whl/cpu` only on
the Linux `cpu` path (`install.rs`'s own `TORCH_CPU_INDEX`; not exercised
here, since this run used the plain macOS/arm64 `pip_calls` path, see the
gap note below).

## Platform

- Date: 2026-10-12.
- `python3.13 -m venv`, pip 26.2, on macOS 26.6.2 / arm64 (Apple Silicon).
- This matches one of Kev's two supported interpreter versions
  (`crates/ariadne-daemon/src/ai_permissions/python.rs:14`,
  `SUPPORTED: [(3, 12), (3, 13)]`) and the platform this audit's sandbox
  actually runs on.

## Files

- `dryrun-stdout.log` — pip's own stdout for the command above (sanitized:
  contains no credentials, only public package names/versions/URLs).
- `resolved-packages.txt` — the 76 resolved `(name, version)` pairs
  (`kev` itself plus 75 dependencies), parsed from `--report`'s structured
  JSON (not scraped from the stdout log), one per line.
- `osv-querybatch-raw-response.json` — the verbatim HTTP response body of
  one `POST https://api.osv.dev/v1/querybatch` request covering all 76
  packages at their exact resolved versions, in request order.
- `osv-results-normalized.json` — the same result, reshaped into one
  `{name, version, advisory_ids}` record per package for readability
  (empty `advisory_ids` for 75 of them; 8 ids for `torch`). This is a
  derived view, not the raw response — see the file above for that.

## Coverage gaps this evidence does not close

- **Platform/device combinations not exercised.** This dry-run took the
  macOS `pip_calls` path (`install.rs`'s `pip_calls`: "On macOS the one
  wheel serves every device"). The Linux `cpu` index override
  (`TORCH_CPU_INDEX`) and the Linux `cuda` path were not separately
  resolved — `torch`'s version constraint (`>=2.6,<2.9`) is identical on
  every platform `install.rs` targets, so the vulnerable range is the same,
  but this evidence does not independently confirm the exact resolved
  version pip would choose on Linux (CPU or CUDA) or under Python 3.12
  (only 3.13 was run here).
- **Laya's own dependency tree was not similarly resolved or scanned.**
  Laya is a benchmark-only dependency (SUPPLY-001); this evidence covers
  only Kev, the production dependency.
- **Build-isolation vs. target-environment installs are not distinguished
  by this evidence.** `setuptools` appears in the resolved set; corrected
  from an earlier draft of this note, which guessed it came from Kev's own
  `[build-system] requires`. `dryrun-stdout.log:41` shows the real source:
  `Collecting setuptools (from torch<2.9,>=2.6->kev @ ...)` — it is a
  runtime dependency **of `torch` itself**, not of Kev's build backend.
  Whether pip's real (non-dry-run) install of Kev's own build-system
  requirement additionally installs a `setuptools` into an isolated build
  environment, separate from this one in the target venv, was not traced
  here.
- **This is a dependency-resolution and advisory-database check, not an
  exploit run.** No checkpoint file was crafted and no code was executed
  against any resolved package to confirm `CVE-2026-24747` is exploitable
  in Kev's actual call pattern; see the main report's SUPPLY-007 for what
  is and is not claimed from this evidence.
