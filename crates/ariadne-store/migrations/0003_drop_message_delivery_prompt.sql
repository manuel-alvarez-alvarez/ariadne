-- The notice an agent is woken with when a message addresses it is the
-- daemon's own plumbing now, not a prompt kind a profile owns: it is rendered
-- from a constant in `ariadne-daemon` and there is nothing left to edit. Any
-- override somebody wrote for it is text no code will ever read again, so it
-- goes rather than sitting in the table as a kind Rust no longer knows.
DELETE FROM profile_prompts WHERE kind = 'message_delivery';
