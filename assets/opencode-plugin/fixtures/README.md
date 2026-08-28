# Captured message list

`opencode-messages.json` is what `client.session.messages()` returned for one
real OpenCode session — captured on this project's machine on 2026-08-28
(opencode 1.18.15, `opencode run` against a local `ollama/qwen3.6-code`) and
reduced to the fields [`../ariadne-events.js`](../ariadne-events.js) reads. The
`parts` are kept as empty arrays because the plugin never looks at them; they
held the prompt, the file it read and the machine's paths.

OpenCode's own figure for that session, from `GET /session/{id}` — the same
counters it stores in `session.tokens_*`:

```json
{ "input": 14812, "output": 140, "reasoning": 0, "cache": { "read": 0, "write": 0 } }
```

which the plugin's arithmetic turns into `input_tokens` 14812,
`cached_input_tokens` 0, `output_tokens` 140. The repository has no JS test
runner, so that comparison is made by hand rather than by a test.

The session ran on a local model, so its cache and reasoning counters are zero
and the fixture cannot demonstrate that arm of the contract. What settles it is
OpenCode's own bookkeeping rather than this capture — see the comment above
`sumUsage` in the plugin.
