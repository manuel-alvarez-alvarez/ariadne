With `code-review`:
- Read the whole change with `get_diff`.
- On a must-fix finding, return the work with `fail_step` and the return report.
- Else pass the work with `complete_step`, with the verdict and the optional findings.
