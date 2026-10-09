-- A task's author keeps the request it opened until a human merges or
-- closes it (005, 026), so a request of the user gets no session of its own
-- any more, and the pin such a session ran on goes with it. A request of
-- the user that no task opened is not Ariadne's to keep: its row goes, and
-- its sessions, comments and their events go with it by cascade.
DELETE FROM agent_sessions WHERE pull_request_id IS NOT NULL AND seat = 'author';
DELETE FROM pull_requests WHERE role = 'author' AND origin_task_id IS NULL;
ALTER TABLE forge_integrations DROP COLUMN babysit_model;
ALTER TABLE forge_integrations DROP COLUMN babysit_effort;
