-- Whether this repository's agents hold what they post to the forge for a
-- person's approval, pull requests opened as drafts included (002, 026).
-- Every existing row starts supervised: an upgrade holds posts rather than
-- exposing them by surprise.
ALTER TABLE repositories ADD COLUMN supervise_posts INTEGER NOT NULL DEFAULT 1
                          CHECK (supervise_posts IN (0, 1));
