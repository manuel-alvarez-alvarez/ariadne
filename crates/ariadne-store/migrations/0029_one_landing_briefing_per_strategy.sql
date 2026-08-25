-- The landing briefing splits in two, one per merge strategy.
--
-- `landing_instructions` was one text carrying both procedures, and the
-- engineer reading it was told to follow the section naming its repository's
-- strategy and ignore the other. The daemon knows the strategy when it
-- renders, so there are two kinds now — `landing_direct` and
-- `landing_pull_request` — and what reaches an engineer is the procedure it
-- runs, whole, with nothing of the other in it.
--
-- Since migration 0028 a stored prompt is an override and nothing else, so
-- there is no default to rewrite here: the only rows this table can hold for
-- the retired kind are ones a user wrote against a text that no longer exists
-- in that shape — half of it about the strategy their repository does not
-- have. They are dropped, and both new kinds start on the defaults the code
-- ships, which `ariadne profile prompt set` writes over the moment somebody
-- wants their own again.
DELETE FROM profile_prompts
WHERE kind = 'landing_instructions';
