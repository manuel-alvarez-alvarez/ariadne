-- The agent CLI and model a task, a reviewer slot or a goal was created with.
--
-- Until now these lived only on the profile, so editing a profile silently
-- moved every task already defined against it — including tasks mid-flight,
-- which would resume on a different agent than they started on. Creation
-- snapshots the profile's agent_kind/model onto the row instead, and the row
-- is what the launcher reads from there on: a profile edit only reaches work
-- created after it.
--
-- Both columns are nullable because both NULLs are meaningful pins, exactly
-- as on `profiles`: NULL agent_kind means auto (resolved at spawn time to the
-- first installed CLI), NULL model means the agent CLI's own default.
--
-- `system_prompt` is deliberately not pinned: rewording a briefing is meant to
-- reach running work.

ALTER TABLE tasks ADD COLUMN agent_kind TEXT
    CHECK (agent_kind IN ('claude_code', 'codex', 'opencode'));
ALTER TABLE tasks ADD COLUMN model TEXT;

ALTER TABLE task_reviewers ADD COLUMN agent_kind TEXT
    CHECK (agent_kind IN ('claude_code', 'codex', 'opencode'));
ALTER TABLE task_reviewers ADD COLUMN model TEXT;

ALTER TABLE goals ADD COLUMN agent_kind TEXT
    CHECK (agent_kind IN ('claude_code', 'codex', 'opencode'));
ALTER TABLE goals ADD COLUMN model TEXT;

-- Rows that predate the columns are pinned to what they resolve to today, so
-- the upgrade itself changes nothing: they keep running on the profile they
-- were running on, and stop following it after the next edit.
UPDATE tasks SET
    agent_kind = (SELECT p.agent_kind FROM profiles p WHERE p.id = tasks.engineer_profile_id),
    model      = (SELECT p.model      FROM profiles p WHERE p.id = tasks.engineer_profile_id);

UPDATE task_reviewers SET
    agent_kind = (SELECT p.agent_kind FROM profiles p WHERE p.id = task_reviewers.profile_id),
    model      = (SELECT p.model      FROM profiles p WHERE p.id = task_reviewers.profile_id);

UPDATE goals SET
    agent_kind = (SELECT p.agent_kind FROM profiles p WHERE p.id = goals.planner_profile_id),
    model      = (SELECT p.model      FROM profiles p WHERE p.id = goals.planner_profile_id);
