-- A workflow or a skill may now be deleted once every goal naming it has
-- ended (`completed` or `cancelled`): a terminal goal never again staffs a
-- task off `goal_steps`'s own snapshot or an already-staffed agent's row, so
-- a name either still holds that the catalog has lost already is no loss at
-- all. The hard foreign keys on `goals.workflow` and
-- `task_agent_skills.skill_name` would otherwise refuse that delete at the
-- database level regardless of status, so both are dropped here. SQLite's
-- `ALTER TABLE` cannot drop a column constraint, so each table is rebuilt
-- whole, the way `0002_workflow_rank_bands.sql` rebuilt `goal_steps`.
-- `Store::open` runs every migration with foreign keys off and turns them
-- back on once they are done, so this file states no `PRAGMA` of its own.

CREATE TABLE goals_new (
    id           TEXT PRIMARY KEY,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'planning'
                 CHECK (status IN ('planning', 'active', 'completed', 'cancelled')),
    orchestrated INTEGER NOT NULL DEFAULT 1 CHECK (orchestrated IN (0, 1)),
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    model        TEXT NOT NULL,
    effort       TEXT,
    issue_url    TEXT,
    -- The workflow every task of the goal runs on. Chosen once, when the goal
    -- is created; its columns are snapshotted into `goal_steps`. No longer a
    -- foreign key: a terminal goal may outlive the catalog row it named.
    workflow     TEXT NOT NULL,
    orchestrator_answered_failed_task_ids TEXT,
    orchestrator_given_up_at TEXT,
    orchestrator_given_up_wedged INTEGER NOT NULL DEFAULT 0
);

INSERT INTO goals_new SELECT * FROM goals;

DROP TABLE goals;
ALTER TABLE goals_new RENAME TO goals;

-- The skills an agent loads, in the order they are listed to it. No longer a
-- foreign key on `skill_name`: an agent staffed from a terminal goal's own
-- row may keep naming a skill already deleted.
CREATE TABLE task_agent_skills_new (
    agent_id   TEXT NOT NULL REFERENCES task_agents (id) ON DELETE CASCADE,
    skill_name TEXT NOT NULL,
    ordinal    INTEGER NOT NULL,
    PRIMARY KEY (agent_id, skill_name)
);

INSERT INTO task_agent_skills_new SELECT * FROM task_agent_skills;

DROP TABLE task_agent_skills;
ALTER TABLE task_agent_skills_new RENAME TO task_agent_skills;
