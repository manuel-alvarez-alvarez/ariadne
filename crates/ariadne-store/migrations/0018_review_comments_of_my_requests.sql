-- Whether an Ariadne review session posted the comment (029). On a request
-- of the user's own such a comment is posted under their login, yet it is a
-- finding for the task's author to answer: it waits on that author and is
-- told to it as a comment of another login would be. A detail fetch keeps
-- the mark; nothing but a review session's own post sets it.
ALTER TABLE pull_request_comments ADD COLUMN from_review INTEGER NOT NULL DEFAULT 0;
