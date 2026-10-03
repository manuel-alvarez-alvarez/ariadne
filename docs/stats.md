# Stats

See what the work did. Stats count what happened as the work ran: each
session that ended, each task that ended, each switch, review, tool call and
permission answer. The daemon writes one record of each such event to a
ledger in its database, and every stat adds up those records.

The stats come in six families, and each one answers one question:

| Family | Question |
| --- | --- |
| `work` | What got done? |
| `time` | How long does it take? |
| `spend` | What did it spend? |
| `models` | Which model does the job? |
| `attention` | How much did it need me? |
| `tools` | What do the agents do? |

The records outlive the work. A stat still counts a session after you delete
the goal it belonged to.

## Filter the stats

Every family takes the same two filters:

- `since`: only what happened since then. Give a span back from now — a whole
  number and `m` (minutes), `h` (hours), `d` (days) or `w` (weeks), such as
  `24h`, `7d` or `30d` — or an RFC 3339 moment, such as
  `2026-09-01T00:00:00Z`. Without it, the stats count everything.
- `repo`: only what happened in one repository.

A chart over time draws one bar per day for a span of 31 days or less, and one
bar per week (from Monday, UTC) for a longer span or for all time.

## From the CLI

```sh
ariadne stats                       # the same as `ariadne stats work`
ariadne stats time --since 7d
ariadne stats spend --repo <repo-id> --format json
```

The families are `work`, `time`, `spend`, `models`, `attention` and `tools`.
`--repo` takes a repository id or a unique prefix of one. Each family takes
the table flags (`--no-trunc`, `-o`, `--columns`), and `--format json` prints
what the daemon sent, whole. A family with nothing to show prints one
sentence that says so.

## In the desktop app

Open **Stats** in the sidebar. Choose a span (all time, 24 hours, 7 days or 30
days) and a repository at the top of the screen; both stay in the address, so
a reload keeps them. The screen shows one section per family, in the order of
the table above, each under its question. The sections update by themselves
as sessions end and as tasks move.

## From the API

`GET /v1/stats/<family>?since=7d&repo=<repo-id>` returns one family, such as
`GET /v1/stats/work`. A `since` that is neither a span nor a moment returns
`400` with the code `invalid_request`.

## Work

Its task writes this section.

## Time

Its task writes this section.

## Spend

Its task writes this section.

## Models

Its task writes this section.

## Attention

Its task writes this section.

## Tools

Its task writes this section.
