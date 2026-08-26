-- What each agent session has spent, as the transcripts under it report it.
--
-- A `source` is one transcript the totals were read from — the JSONL file a
-- Claude session writes, a Codex rollout, an OpenCode session — and its three
-- counters are that transcript's *cumulative* totals, never a delta: a fresh
-- report for the same source replaces the previous one. A session accumulates
-- several sources when its agent is resumed into a new transcript, so the
-- session's usage is the sum over its rows, and a task's or a goal's is the
-- sum over the sessions under it.
--
-- `input_tokens` counts every prompt token, cache reads and cache writes
-- included, and `cached_input_tokens` is the subset of it served from the
-- prompt cache — so the two are never added together. `output_tokens` counts
-- completion tokens, thinking and reasoning included.
CREATE TABLE session_usage (
    session_id          TEXT NOT NULL REFERENCES agent_sessions (id) ON DELETE CASCADE,
    source              TEXT NOT NULL,
    input_tokens        INTEGER NOT NULL,
    cached_input_tokens INTEGER NOT NULL,
    output_tokens       INTEGER NOT NULL,
    updated_at          TEXT NOT NULL,
    PRIMARY KEY (session_id, source)
);
