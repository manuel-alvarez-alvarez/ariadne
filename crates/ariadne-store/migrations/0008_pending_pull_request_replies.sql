-- Whether a comment's reply is held, unposted, in the user's own forge
-- drafts (026 rule 24): a one-way local mark, never read back off the
-- forge, so a draft deleted there before the user publishes it leaves the
-- comment answered all the same.
ALTER TABLE pull_request_comment_marks ADD COLUMN pending_reply INTEGER NOT NULL DEFAULT 0
                                        CHECK (pending_reply IN (0, 1));
