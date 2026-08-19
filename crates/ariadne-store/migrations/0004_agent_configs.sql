-- Per-agent-kind launch configuration, replacing the per-profile flag list.
--
-- The permission-bypass flags used to be hardcoded in the agent adapters and
-- each profile carried an `extra_flags` list of its own. Both were the wrong
-- place: how an agent CLI is allowed to run is a property of that CLI, not of
-- the persona a profile describes, and hardcoding it left the user nothing to
-- turn off. One row per agent kind holds it now, read on every spawn and
-- resume.
--
-- Schema only, as in 0001: the rows are seeded from Rust
-- (`ariadne_core::AgentKind::default_flags`) after the migrations run, so the
-- defaults have a single source of truth and an agent kind added later gets
-- its row without a migration.

CREATE TABLE agent_configs (
    agent_kind  TEXT PRIMARY KEY CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    extra_flags TEXT NOT NULL,                  -- JSON array of argv strings
    updated_at  TEXT NOT NULL
);

ALTER TABLE profiles DROP COLUMN extra_flags;
