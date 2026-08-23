-- Who a message is addressed to.
--
-- A message has always had an author; it could never name an addressee, so a
-- reply to one agent in a thread several agents read was addressed only in
-- prose. The recipient is what a wake path can act on later: one agent, or
-- the human user.
--
-- NULL `recipient_kind` is the shape every message had before this column and
-- keeps its meaning — said to the thread, addressed to nobody in particular.
-- A profile addressee carries its id; the user has none, hence the two checks
-- tying the id to the kind (written with `IS`, since a comparison against a
-- NULL kind would be NULL, and a NULL check passes).

ALTER TABLE messages ADD COLUMN recipient_kind TEXT
    CHECK (recipient_kind IN ('profile', 'user'));
ALTER TABLE messages ADD COLUMN recipient_profile_id TEXT REFERENCES profiles (id)
    CHECK (recipient_profile_id IS NULL OR recipient_kind IS 'profile')
    CHECK (recipient_kind IS NOT 'profile' OR recipient_profile_id IS NOT NULL);
