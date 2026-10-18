-- A request can run a reviewer and a keeper at the same time. Their
-- automatic-recovery decisions are independent: a keeper launch must not
-- clear the reviewer's recovery item, and a reviewer launch must not clear
-- the keeper's. Keep each seat's first give-up time and cause on the row.

ALTER TABLE pull_requests ADD COLUMN keeper_given_up_at TEXT;
ALTER TABLE pull_requests ADD COLUMN keeper_given_up_wedged INTEGER NOT NULL DEFAULT 0;
