With `merge`:
- The primary checkout is the `Repo:` path.
- Pass the work with `complete_step`, with the report and that sha as `merge_commit`.
- On a failed check of a task file, call `fail_step` with the check's output as the return report.
