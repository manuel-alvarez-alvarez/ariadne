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

## From the CLI

```sh
ariadne stats                       # the same as `ariadne stats models`
ariadne stats models --since 7d
ariadne stats models --repo <repo-id> --format json
ariadne stats switches --since 7d
ariadne stats tools --since 7d
```

`--repo` takes a repository id or a unique prefix of one. The table prints
tokens in the same form as every other table, such as `↑1.2M 89.1% ↓45k`, and
takes the table flags (`--no-trunc`, `-o`, `--columns`). `--format json`
prints the rows that the daemon sent.

## In the desktop app

Open **Stats** in the sidebar. Choose a span (all time, 24 hours, 7 days or 30
days) and a repository at the top of the screen; both stay in the address, so
a reload keeps them. The **Models** panel shows the same rows as
`ariadne stats models`, and the **Switches** panel the same rows as
`ariadne stats switches`; both update by themselves as sessions end and
switch.

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
over every one of them. A `since` that is neither a span nor a moment returns
`400` with the code `invalid_request`.

`GET /v1/stats/reviews?since=7d&repo=<repo-id>` returns author, reviewer and
message aggregates.
`GET /v1/stats/tools?since=7d&repo=<repo-id>` returns tool, model and
permission aggregates.
