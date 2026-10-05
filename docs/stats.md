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

```sh
ariadne stats work                  # the totals, then a table of the buckets
ariadne stats work --format json
```

## Time

Time shows how long finished tasks took from creation to their ending. It
shows the median, p90 and mean lead time, then breaks time down by task status:
pending, ready, in progress, under review, changes requested and approved.
Each status has its total, median and share of all recorded status time.

It also shows how long work waited for a person to answer a permission prompt.
Only console decisions count there; automated, learned and AI decisions do
not wait on a person. The CLI prints the headline lead and waiting figures,
then a status table. The Stats screen shows the same headline figures and a
"Where the time goes" chart with each status's median and share.

## Spend

What did it spend: tokens over time, by model, and per finished task. Stats
show tokens, never cost — nothing here converts one to the other.

The totals are every ended session's tokens: how much went in, how much of
that the prompt cache served, and how much came out. Per finished task is the
same two figures — in and out — averaged over however many tasks finished in
the span.

The chart over time stacks input and output tokens per bucket, the cached
share alongside in the tooltip. The chart by model is one bar per model that
ran, every seat it ran in pooled together, each bar's share of the whole
spend in its tooltip.

## Models

Compare models within the same seat to choose who should do the next job.

```sh
ariadne stats models
ariadne stats models --seat author --since 7d
ariadne stats models --format json
```

The command shows all seats by default.
Use `--seat author`, `--seat reviewer`, or `--seat orchestrator` to show one table.
JSON always returns every seat, even with `--seat`.
The desktop shows Authors, Reviewers, and Orchestrators, with the largest task, verdict, or session count first.
Seats without rows have no table.
When no author, reviewer, or orchestrator rows exist, the desktop shows its empty state.
This includes responses with only seatless rows.

Authors show task endings, finish rate, first-pass rate, average review rounds, contest win rate, and tokens per finished task.
TASKS includes finished, failed, and cancelled endings; retries can contribute more than one ending.
Finish rate is finished endings divided by finished, failed, and cancelled endings combined.
First-pass rate is reviewed tasks with a round-one approval divided by all reviewed tasks for that author model.
Any reviewer's round-one approval qualifies, and each task counts once.
ROUNDS averages review requests across task endings.
Win rate is contests won divided by contests entered, counting a model only once per contest.
TOKENS/TASK averages author-session input plus output across distinct finished tasks attributed to that model.
It includes zero tokens for finished tasks without matching sessions.

Reviewers show verdict count, approval share, and average time from the review request to the verdict.
Approve share is approval verdicts divided by all verdicts.
Orchestrators show session count, input plus output tokens, and average session lifetime.
Every table shows failed sessions and switches caused by exhaustion under FAILED and EXHAUSTED.

Every table also shows how often a person stepped in for the model, and how long its sessions ran.
INTERVENTIONS counts three things:

- permissions that you answered at the console (an AI or learned answer does not count),
- questions, where the agent waited for your input,
- stalls, where the agent stalled or failed with an error.

A session that waits on a permission counts once, as the permission.
In the desktop, hover over or focus the INTERVENTIONS figure to see the three counts and the person time.
Person time is how long those prompts and questions waited on you.
TOTAL_TIME is the sum of the session lifetimes.
Authors also show INTERVENTIONS/TASK, the interventions per finished task, and `-` without a finished task.
The CLI also prints PERSON_TIME for every seat, and LEAD_TIME, the median lead time of finished tasks, for authors.

The API and JSON also include all three token counts, stalled sessions, cached share, and session lifetime for every seat.
They carry the intervention breakdown under `interventions` and the median lead time of an author under `author.median_lead_time_secs`.
Cached share is cached input tokens divided by input tokens; cached tokens are already included in input.
Rates and averages without observations are zero.
Both filters apply to every contributing record, including records matched by task.
A short span can exclude an earlier session or ending and change the averages.
Seatless sessions appear under NONE in the CLI and in JSON.

## Attention

**Attention** answers how much the work needed you. It shows permission
prompts by who answered them and whether they were allowed, denied or
cancelled, along with the average wait. It also shows questions the agents
asked, attention flags by reason, failed and stalled sessions, and switches
made because a model ran out of room.

In the desktop app, the four headline figures are the prompts you answered,
your mean wait, questions asked and stalled sessions. The charts break down
permission answers by decider and attention flags by reason. In the CLI,
`ariadne stats attention` prints those session figures followed by the two
tables; `--format json` returns the full response.

## Tools

What do the agents do? Each ended tool call writes one fact, under the
tool's own name — `Bash`, `Edit`, `Read`, an MCP tool's full name — and the
kind of thing it did: `read`, `edit`, `delete`, `move`, `search`, `execute`,
`think`, `fetch`, `switch_mode` or `other`.

`ariadne stats tools` prints the total calls and errors, a table of the
calls by kind, and a table of the tools with the most calls, the rest summed
into one `other` row. `--limit` sets how many tools the table — and the
desktop app's chart — show, 1 to 100, 10 by default. The Tools section of
the desktop app draws the same two groupings as bar charts, ok and errors
stacked, with the median and the p90 call duration in the tooltip.

A tool call recorded before this existed has no kind of its own, and does
not count.
