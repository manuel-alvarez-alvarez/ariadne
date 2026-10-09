-- The pin the user picked when they asked Ariadne to review a request of
-- their own (029): the review session runs on it rather than on the
-- repository's review pin. NULL where nobody asked.
ALTER TABLE pull_requests ADD COLUMN review_model TEXT;
ALTER TABLE pull_requests ADD COLUMN review_effort TEXT;
-- The skills the user picked for that review beside `pr-reviewer`, which a
-- review session always loads: a JSON list of skill names.
ALTER TABLE pull_requests ADD COLUMN review_skills TEXT NOT NULL DEFAULT '[]';
