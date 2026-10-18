# specs/AGENTS.md

Conventions for writing and changing a spec. Commit-message and history
rules live in the root [`AGENTS.md`](../AGENTS.md). What a spec is for, and
the index of every one, are in [`README.md`](README.md).

## Writing one

- One file per subsystem, `NNN-kebab-slug.md`, numbered in the order they were
  written. A number is never reused: a subsystem that goes away leaves its
  number behind, so a reference in the git history still points where it did.
- YAML frontmatter: `id`, `status`, `updated`, `areas`, `commits`, `tests`.
  `areas` uses the same names as a commit's scope (root
  [`AGENTS.md`](../AGENTS.md#commit-messages)).
- Sections: **Scope** (in and out), **Behavior** (numbered rules), **Acceptance
  criteria** (each citing the test that proves it), **Sources**. Add **Known
  gap** where something is deliberately unbuilt.
- A rule belongs to exactly one spec. Where another needs it, reference the
  number rather than restating it.
- Update the spec in the same change as the code. A spec that disagrees with
  its tests is a bug in one of the two.
- A spec that a later one replaces whole is marked `status: superseded`, names
  the spec that holds the account now, and keeps its file: its number and its
  text stay, so a reference in the git history still resolves.
- The index row in [`README.md`](README.md) changes in the same commit as the
  spec it describes — a new spec, a superseded one, or a changed one-line
  summary.

## Checks

Every test a spec's frontmatter or acceptance criteria cites resolves to a
real test in the tree.
