-- A request the user is a requested reviewer of gets a session of its own
-- (029). Every change adds a column beside the ones that exist, so an older
-- row keeps every value it has.

-- The head the reviewer session last posted a review on, as it reports it.
ALTER TABLE pull_requests ADD COLUMN reviewed_sha TEXT;
-- The head the session was last told of, so a push is told once. NULL is
-- the baseline of a row no session was started on.
ALTER TABLE pull_requests ADD COLUMN told_head_sha TEXT;
-- Whether the last repository fetch listed the request as one that asks
-- for the user's review. A request read on its own, since no list held it,
-- lost that request.
ALTER TABLE pull_requests
    ADD COLUMN review_requested INTEGER NOT NULL DEFAULT 1 CHECK (review_requested IN (0, 1));
