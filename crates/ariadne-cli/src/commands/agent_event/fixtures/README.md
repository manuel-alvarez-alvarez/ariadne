# Captured transcripts

One real record per agent, captured on this project's machine on 2026-08-28 and
reduced to the fields [`usage.rs`](../usage.rs) reads. Everything else — the
prompts, the file contents, the absolute paths, the rate-limit and account
details — was dropped before committing; line order, line count and the
`type`/`payload` shells around the counters are the ones the agent wrote, so
each fixture still walks the reader's real path.

| Fixture | Captured by | What the agent said it spent |
| --- | --- | --- |
| `claude-session.jsonl`, `claude-subagent.jsonl` | `claude -p --output-format json --model sonnet` (claude 2.1.251), one Read plus one `Task` subagent | `modelUsage["claude-sonnet-5"]`: 10 input, 55108 cache read, 19473 cache creation, 1059 output |
| `codex-rollout.jsonl` | `codex exec` then `codex exec resume --last` (codex-cli 0.150.1), both at `model_reasoning_effort=high` | last `total_token_usage` per process: 27240/24064/280 and 14490/11008/154; printed `tokens used` 3456 and 3636 |

The OpenCode fixture lives beside its own reader, at
[`assets/opencode-plugin/fixtures/`](../../../../../../assets/opencode-plugin/fixtures/).

One line of `codex-rollout.jsonl` is not from that rollout, and is marked here
rather than in the file because JSONL carries no comments: the `"info": null`
`token_count`, a rate-limit-only update the reader has to skip. No rollout from
this week contained one — 5 of the ~500 on this machine do — so it was taken
from a real rollout of 2026-08-21 and stripped of its `rate_limits` body. It
carries no counters, so it changes no total.
