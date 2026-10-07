# Invalidated pilot — do not cite

These four files are the first handoff-selection run, kept only as a record of
what was tried. Do not use them as evidence for or against adoption.

Why they are invalid:

- `run.py`'s `rules(item)` read `item["labels"]`. A label is an answer
  annotation, not evidence a production selector would ever see. The
  "rules" baseline in `results.json` is an oracle, not a deployable policy.
- `case_data()` built every history from six fixed fact texts, repeated
  across all 60 cases with only the id and family name changed. The
  "development" and "evaluation" splits were not independent histories.
- The cold-start timer in `execute()` started before `kev_backend()` and
  stopped after the whole scoring loop, so `cold_start_ms` in `results.json`
  included warm inference time, not model load alone.
- `report.md` and `results.json` were written from different runs and do
  not describe the same measurement (different latency and memory figures).

The repair lives in the parent directory: `dataset.py` builds genuinely
distinct histories with neutral entry ids and annotations kept out of the
policy input, `run.py` scores a deployable rules baseline from observable
text and keeps an annotation-based oracle as a separate upper bound, and
`results.json`/`report.md` there describe one measured run.
