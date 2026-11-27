-- The facts of a notice now name its severity (spec 040). A notice stored
-- before this migration has facts without it, so its facts are cleared and
-- the facts fill runs again, on these rows alone.
UPDATE agent_events SET facts = NULL WHERE kind = 'notice';
DELETE FROM store_fills WHERE name = 'agent_event_facts';
