-- A request of the user's own can be reviewed by Ariadne too, on the user's
-- asking (029): a review session as one that asks for the user's review
-- gets. Off on every row: nobody asked yet.
ALTER TABLE pull_requests
    ADD COLUMN review_asked INTEGER NOT NULL DEFAULT 0 CHECK (review_asked IN (0, 1));
