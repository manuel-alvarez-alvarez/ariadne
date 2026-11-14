-- An Ariadne approval of a request (029): its review session found the
-- request fit to merge on one exact head. The approval is Ariadne's own
-- status, never a forge approval.
--
-- It is keyed by the request's repository and number, not by
-- `pull_requests.id`, so it outlives the row: stopping a review or a
-- daemon restart keeps it while the head stays the same. A head that moves
-- deletes it, and the same head coming back does not restore it.
-- Recovery: `DROP TABLE pull_request_approvals;` loses only approvals.
CREATE TABLE pull_request_approvals (
    repository_id  TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    number         INTEGER NOT NULL CHECK (number > 0),
    head_sha       TEXT NOT NULL,
    approved_at    TEXT NOT NULL,
    PRIMARY KEY (repository_id, number)
);
