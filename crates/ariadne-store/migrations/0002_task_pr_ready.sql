-- Whether a published request's approvals and checks last read ready to
-- merge. Publishing alone leaves this 0; it clears on a new change or a
-- failed check, so a later ready report raises the notice again.
ALTER TABLE tasks ADD COLUMN pr_ready INTEGER NOT NULL DEFAULT 0;
