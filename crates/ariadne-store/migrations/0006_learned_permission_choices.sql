-- Learned permissions become training data for the permission model: one row
-- per user choice and per denial, allow or deny, keyed by the repository, the
-- tool name and the canonical `rawInput` of the ACP `toolCall`. The old rows
-- hold only approvals under another key, so none is carried over.
DROP TABLE learned_permissions;

CREATE TABLE learned_permissions (
    id              TEXT PRIMARY KEY,
    repository_id   TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    tool_name       TEXT NOT NULL,
    tool_call       TEXT NOT NULL,              -- the ACP `toolCall`, `rawInput` with sorted keys
    options         TEXT NOT NULL,              -- the ACP `options`
    selected_option TEXT NOT NULL,              -- the option id of the final choice
    target          TEXT NOT NULL CHECK (target IN ('auto', 'ask', 'learn', 'ai')),
    output          TEXT,                       -- the model decision, when the model was called
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_learned_permissions_repository
ON learned_permissions (repository_id, created_at DESC);

CREATE UNIQUE INDEX idx_learned_permissions_key
ON learned_permissions (repository_id, tool_name, ifnull(tool_call -> '$.rawInput', 'null'));
