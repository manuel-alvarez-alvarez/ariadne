# Stats

See how Ariadne and the models perform. Stats count what happened as the work
ran: each session that ended, which model and seat it ran on, what it spent and
how long it lived. The daemon writes one record of each such event to a ledger
in its database, and every stat adds up those records.

The records outlive the work. A stat still counts a session after you delete
the goal it belonged to.

## Filter the stats

Every stat takes the same two filters:

- `since`: only what happened since then. Give a span back from now — a whole
  number and `m` (minutes), `h` (hours), `d` (days) or `w` (weeks), such as
  `24h`, `7d` or `30d` — or an RFC 3339 moment, such as
  `2026-09-01T00:00:00Z`. Without it, the stats count everything.
- `repo`: only what happened in one repository.

## Models

The `models` stat has one row for each model in each seat (orchestrator,
author, reviewer). A row shows:

- `sessions`: the session runs that ended. A session that you resume and that
  ends again counts once for each run.
- `failed`: the runs that ended `failed`.
- `stalled`: the runs that ended flagged as stalled.
- tokens: what those runs spent — input, the share of the input the prompt
  cache served, and output. Stats show tokens, not cost.
- lifetime: the mean time from a session's start to its end.
- skills: each skill those runs loaded, and how many runs loaded it.

## Switches

The `switches` stat has one row for each model that a session left or
arrived on when it was switched to another model or agent (by hand, or by the
daemon itself once a model is exhausted). A row shows:

- `switches`: sessions that left this model.
- `exhausted`: of those, the ones that left because the model was exhausted.
- `automatic`: the share of those switches the daemon made by itself, from 0
  to 100%.
- `arrivals`: sessions that arrived on this model from another.

## Outcomes

The `outcomes` stat has one row for each author model, and a totals row
summing every model. A task writes the fact a row is built from once it
reaches `finished`, `cancelled` or `failed`; the model, effort and skills on
that row are the task's picked author where several wrote it, or its one
author where only one did. A row shows:

- `finished`, `failed`, `cancelled`: how many of that model's tasks ended
  each way, and `finish_rate`: `finished` over all three.
- lead time: the mean and the median time from a task's creation to its
  ending.
- `mean_review_requests`: the mean count of review requests a task of that
  model's sent before it ended.
- a contest: several authors wrote the same task side by side and the
  reviewers picked the one that lands. `contests_entered` counts every
  contest that model's author was staffed in, win or lose; `contests_won`
  counts the ones it won; `win_rate` is the one over the other. Two authors
  of one contest on the same model count as one entry for it, not two.

The totals row is the sum of the rows above it, not a count of its own: it
holds nothing that is not on one of them.

## From the CLI

```sh
ariadne stats                       # the same as `ariadne stats models`
ariadne stats models --since 7d
ariadne stats models --repo <repo-id> --format json
ariadne stats switches --since 7d
ariadne stats tools --since 7d
ariadne stats outcomes --since 30d
```

`--repo` takes a repository id or a unique prefix of one. The table prints
tokens in the same form as every other table, such as `↑1.2M 89.1% ↓45k`, and
takes the table flags (`--no-trunc`, `-o`, `--columns`). `--format json`
prints the rows that the daemon sent. `stats outcomes` prints one row per
model and a totals row after it.

## In the desktop app

Open **Stats** in the sidebar. Choose a span (all time, 24 hours, 7 days or 30
days) and a repository at the top of the screen; both stay in the address, so
a reload keeps them. The **Models** panel shows the same rows as
`ariadne stats models`, the **Switches** panel the same rows as
`ariadne stats switches`, and the **Outcomes** panel the same rows as
`ariadne stats outcomes`; all of them update by themselves as sessions end,
switch, and as tasks move.

## Reviews

`ariadne stats reviews` shows author approval rounds, reviewer verdict and
latency figures, and message flow by kind and sender. The Stats screen shows
author figures in its **Reviews** panel. It takes the same filters as models.

## Tools and permissions

The `tools` stat shows calls and errors per tool, their median and p90
duration, and calls with mean duration per model. It also groups permission
answers by who decided and by answer, with the total and mean wait. The
**Tools** panel uses the same filters as Models.

## From the API

`GET /v1/stats/models?since=7d&repo=<repo-id>` returns `{"items": [...]}`, one
item per model and seat. `GET /v1/stats/switches` returns `{"items": [...],
"switches": ..., "exhaustions": ...}`, one item per model, with the totals
over every one of them. `GET /v1/stats/outcomes` takes the same filters and
returns `{"items": [...], "totals": {...}}`, one item per author model and the
totals across them. A `since` that is neither a span nor a moment returns
`400` with the code `invalid_request`.

`GET /v1/stats/reviews?since=7d&repo=<repo-id>` returns author, reviewer and
message aggregates.
`GET /v1/stats/tools?since=7d&repo=<repo-id>` returns tool, model and
permission aggregates.
