-- A goal can be Ariadne's work on one pull request: a review today, a
-- babysit later (029, 030). The goal names the request and the operation, so
-- the Goals board shows it and the daemon finds it again after a restart.
--
-- The request is named by value, not by a reference to `pull_requests`: that
-- row goes when Ariadne stops working on the request, and the goal stays as
-- history. All four columns are NULL on an ordinary goal. Every column is
-- new and nullable, so no existing row changes and no caller overlaps.
--
-- Recovery: `ALTER TABLE goals DROP COLUMN` on each column and `DROP INDEX
-- idx_goals_pr_operation_active` restore the old shape without loss.
ALTER TABLE goals ADD COLUMN pr_operation TEXT
    CHECK (pr_operation IN ('review', 'babysit'));
ALTER TABLE goals ADD COLUMN pr_repository_id TEXT;
ALTER TABLE goals ADD COLUMN pr_number INTEGER;
ALTER TABLE goals ADD COLUMN pr_url TEXT;

-- One open goal per request and operation. A completed or cancelled goal is
-- history, so a later start makes a new one.
CREATE UNIQUE INDEX idx_goals_pr_operation_active
    ON goals (pr_repository_id, pr_number, pr_operation)
    WHERE pr_operation IS NOT NULL AND status IN ('planning', 'active');
