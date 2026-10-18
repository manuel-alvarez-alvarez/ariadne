# Stats

See what the work did. Stats count what happened as the work ran: each
session that ended, each task that ended, each switch, review and
permission answer. The daemon writes one record of each such event to a
ledger in its database, and every stat adds up those records.

The stats come in five families, and each one answers one question:

| Family | Question |
| --- | --- |
| `work` | What got done? |
| `time` | How long does it take? |
| `spend` | What did it spend? |
| `models` | Which model does the job? |
| `attention` | How much did it need me? |

The records outlive the work. A stat still counts a session after you delete
the goal it belonged to.

## Filter the stats

Every family takes the same two filters:

- `since`: only what happened since then. Give a span back from now — a whole
  number and `m` (minutes), `h` (hours), `d` (days) or `w` (weeks), such as
  `24h`, `7d` or `30d` — or an RFC 3339 moment, such as
  `2026-09-01T00:00:00Z`. Without it, the stats count everything.
- `repo`: only what happened in one repository.

A chart over time draws one bar per hour for a span of 2 days or less, one bar
per day for a span of 31 days or less, and one bar per week (from Monday,
UTC) for a longer span. Without `since`, the span runs from the earliest
matching record to now, so an all-time chart follows the same rule — weekly
bars only once that record is more than 31 days back.

## From the CLI

The CLI has no `ariadne stats` command any more. Read a family from the API
below. For the daemon's metrics, use `ariadne query` with a PromQL
expression — see [Telemetry](telemetry.md#querying-metrics-and-logs).

## In the desktop app

The desktop app's **Stats** screen reads this ledger through a Perses
dashboard, not a screen of its own over these five families — see
[Dashboards](dashboards.md).

## From the API

`GET /v1/stats/<family>?since=7d&repo=<repo-id>` returns one family, such as
`GET /v1/stats/work`. A `since` that is neither a span nor a moment returns
`400` with the code `invalid_request`.

## Work

What got done: goals completed and cancelled, tasks finished, failed and
cancelled, and changes landed — over time.

The totals:

| Figure | What it counts |
| --- | --- |
| Goals completed | Goals that moved to `completed` |
| Goals cancelled | Goals that moved to `cancelled` |
| Median goal lead time | From a completed goal's creation to its completion |
| Tasks finished / failed / cancelled | Tasks that ended each way |
| Finish rate | Tasks finished, over the three endings |
| Changes landed | Finished tasks whose change was merged or published as a pull request |

The chart below them is tasks per bucket, stacked by how each one ended; its
tooltip also shows goals completed and changes landed for that bucket.

## Time

Time shows how long finished tasks took from creation to their ending. It
shows the median, p90 and mean lead time, then breaks time down by task
status: pending, ready and in progress. Each status has its total, median
and share of all recorded status time.

It also shows how long work waited for a person to answer a permission prompt.
Only console decisions count there; automated, learned and AI decisions do
not wait on a person. The CLI prints the headline lead and waiting figures,
then a status table. The Stats screen shows the same headline figures and a
"Where the time goes" chart with each status's median and share.

## Spend

What did it spend: tokens over time and per finished task. Stats show
tokens, never cost — nothing here converts one to the other. Compare models
by tokens in the Models section instead.

The totals are every ended session's tokens: how much went in, how much of
that the prompt cache served, and how much came out. Per finished task is the
same two figures — in and out — averaged over however many tasks finished in
the span.

The chart over time stacks input and output tokens per bucket, the cached
share alongside in the tooltip.

## Models

Compare models within the same seat to choose who should do the next job.

The answer holds a row per model and seat, the largest task or goal count
first.

Agents show the model's tasks, tokens, time, messages and finished tasks.
TASKS counts the distinct tasks a model ran a session on as a column's
agent; FINISHED counts the ones that column ended `finished`. Orchestrators
show the same figures by goal instead of by task, with no FINISHED column.
TOKENS is input and output tokens together, cached tokens counted once
within input. TIME is the sum of that model's session lifetimes in that
seat, and MESSAGES the count of messages it sent there.

A session with no seat has its own rows too. The API keeps every row,
seatless rows included.

## Attention

**Attention** answers how much the work needed you. It shows permission
prompts by who answered them and whether they were allowed, denied or
cancelled, along with the average wait. It also shows questions the agents
asked, attention flags by reason, failed and stalled sessions, and switches
made because a model ran out of room.

In the desktop app, the four headline figures are the prompts you answered,
your mean wait, questions asked and stalled sessions. The charts break down
permission answers by decider and attention flags by reason.
