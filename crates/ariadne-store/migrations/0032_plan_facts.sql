-- The facts of a plan event now name its ACP plan (spec 040). A plan event
-- stored before this migration has facts without it, so its facts are
-- cleared and the facts fill runs again, on these rows alone.
UPDATE agent_events SET facts = NULL WHERE kind IN ('plan_update', 'plan_removed');
DELETE FROM store_fills WHERE name = 'agent_event_facts';
