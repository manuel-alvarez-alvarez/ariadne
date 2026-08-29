# Captured transcripts

One real record per agent, captured on this project's machine on 2026-08-28
(the Codex sub-agent pair on 2026-08-29) and reduced to the fields
[`usage.rs`](../usage.rs) reads. Everything else — the prompts, the file
contents, the absolute paths, the rate-limit and account details — was dropped
before committing; line order, line count and the `type`/`payload` shells
around the counters are the ones the agent wrote, so each fixture still walks
the reader's real path.

| Fixture | Captured by | What the agent said it spent |
| --- | --- | --- |
| `claude-session.jsonl`, `claude-subagent.jsonl` | `claude -p --output-format json --model sonnet` (claude 2.1.251), one Read plus one `Task` subagent | `modelUsage["claude-sonnet-5"]`: 10 input, 55108 cache read, 19473 cache creation, 1059 output |
| `codex-rollout.jsonl` | `codex exec` then `codex exec resume --last` (codex-cli 0.150.1), both at `model_reasoning_effort=high` | last `total_token_usage` per process: 27240/24064/280 and 14490/11008/154; printed `tokens used` 3456 and 3636 |
| `codex-parent-rollout.jsonl` | the `codex exec` of the pair below, thread `01a04b20-646d-71c2-b14f-4d98e40ae172` | last `total_token_usage` 45845/40192/122; printed `tokens used` 5775 |
| `codex-child-rollout.jsonl` | the sub-agent it spawned, thread `01a04b20-766c-7213-83a2-332002c3af62` | last `total_token_usage` 30645/22016/170, which is 8799 not served from cache; it prints nothing of its own |

The OpenCode fixture lives beside its own reader, at
[`assets/opencode-plugin/fixtures/`](../../../../../../assets/opencode-plugin/fixtures/).

One line of `codex-rollout.jsonl` is not from that rollout, and is marked here
rather than in the file because JSONL carries no comments: the `"info": null`
`token_count`, a rate-limit-only update the reader has to skip. No rollout from
this week contained one — 5 of the ~500 on this machine do — so it was taken
from a real rollout of 2026-08-21 and stripped of its `rate_limits` body. It
carries no counters, so it changes no total.

## How the Codex sub-agent pair was captured

In a scratch directory outside any Ariadne worktree:

```sh
codex exec --sandbox workspace-write --skip-git-repo-check \
  -c features.multi_agent=true \
  'Use the collaboration tool `spawn_agent` (a direct tool call, not through
   exec) to create one sub-agent, and give it this task: "Read NOTES.md in the
   working directory and reply with its contents in one sentence." Then call
   `wait_agent` with a timeout of 120000 to wait for it. When it answers, tell
   me what it said. Do not do the reading yourself.'
```

That wrote two files into `~/.codex/sessions/2026/08/29/`, four seconds apart:
the `codex exec`'s own rollout and the sub-agent's. Their names are kept as
[`usage.rs`](../usage.rs)'s `captured` tests write them out, because a child is
found by the thread id its file name ends with. The last `total_token_usage`
of each was read back before reducing, and is the table's third column; the
`tokens used` the `codex exec` printed as it exited is the parent's line alone,
and 5775 + 8799 is what the two together cost.

Both files keep every `session_meta`, every `SubAgentActivity` and every
`token_count` intact; the `spawn_agent` and `wait_agent` calls keep their
`name`, `namespace` and `output` because what those do *not* carry — the
child's thread id — is why the reader reads `SubAgentActivity` instead.
