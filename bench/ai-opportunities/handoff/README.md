# Session handoff selection benchmark

Run `python3 bench/ai-opportunities/handoff/dataset.py` to rebuild `cases.json` and
`annotations.json` (deterministic; byte-identical on every run).

Run `python3 bench/ai-opportunities/handoff/run.py` for the primary local measurement.
It loads the pinned interpreter's Kev-4B checkpoint under the shared advisory lock,
in the foreground, and holds the lock until the model process releases its memory.

Run `python3 bench/ai-opportunities/handoff/run.py --metrics-only` to recompute committed
prediction metrics with no model, no `torch` import, and no lock.

Run `python3 -m pytest bench/ai-opportunities/handoff/tests -q` for the tests.

`pilot-invalidated/` holds the first run of this benchmark and why it cannot support
adoption; see `pilot-invalidated/NOTE.md`. Do not cite those files as evidence.

## Dataset

`cases.json` is the only file a policy reads: 90 genuinely distinct synthetic histories
(36 development, 54 reserved evaluation), ten scenario templates - a live constraint, a
correction, an unresolved blocker, a decision, a changed-file note, a decisive tool
result, an obsolete constraint superseded by a correction, two entries both needed at
once, a quoted directive planted in tool output, and a run of distracting recent output -
each instantiated nine times with different phrasing and fillers. A case carries a
neutral id, a split, a family, a template, a task, and a history of entries with a
neutral id, a `kind`, and rendered `text`. No case or entry carries a label or a
mandatory marker.

Every template sits wholly in one split, never 6 development / 3 evaluation instances
of the same template: `constraint`, `decision`, `changed_file`, and `tool_result` are
development-only (`dataset.DEV_TEMPLATES`); `correction`, `blocker`,
`obsolete_then_correction`, `multi_mandatory`, `quoted_directive_trap`, and
`distracting_recent_output` are evaluation-only (`dataset.EVAL_TEMPLATES`), which is
where the required coverage - obsolete constraints, corrections, multiple relevant
entries, quoted directives, distracting recent output - lives.

`annotations.json` is graded-only: per case, the mandatory entry ids, the label each
labelled entry carries, and why. `run.py` reads it after a policy has already selected,
never before, and for the `oracle_label` upper bound, which is not a deployable baseline.

A family, and a whole scenario template, stay in one split; `dataset.split_violations`
checks both, off the `family` and `template` fields directly, plus an exact duplicate
rendered history landing in both splits. The committed dataset has none.

## Policies

Policies receive identical character budgets (260, 430, 2000), including introduction
and omission framing. The benchmark copies folded entries, whole-entry removal, and
current recency behavior from `crates/ariadne-daemon/src/agents/handoff.rs`. Selections
keep exact rendered entry text and restore source order.

- `recency` - the production renderer's own policy.
- `rules` - deployable: a score from an entry's `kind` and its folded evidence
  (`run.py:rules`, via `run.py:evidence`), the only evidence a production selector
  would have.
- `oracle_label` - not deployable: the ground-truth mandatory set from
  `annotations.json`, reported only as an upper bound.
- `kev` - `jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`, scored on that
  same folded evidence (`run.py:evidence`) and the same budgets as every other policy.
  A long tool entry's lines beyond the fold are invisible to Kev exactly as they are to
  recency's and rules' budgets.

The runner locks the shared path before loading Kev and releases it when the foreground
process exits. It reads no `crates/` file at runtime. A per-entry call that times out, or
returns a non-finite score, is counted and dropped; a case missing any score falls back
to recency. Model load is timed separately from the scoring loop that follows it.

A later integration can select after `render_entry` and before `apply_budget`, retaining
event ids and user-prompt provenance.
