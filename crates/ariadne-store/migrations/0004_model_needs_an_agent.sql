-- What a goal, a task, a reviewer slot or a profile runs on is chosen as one
-- string now — `<agent_kind>[:<model>]`, the agent CLI and, after a colon, the
-- model of it — so a model with no agent CLI beside it has no spelling left:
-- the HTTP surface takes one `model` field and refuses anything that names no
-- agent. Rows written before that could hold exactly that pair (a NULL
-- `agent_kind`, which is auto, and a model of no CLI in particular), and
-- nothing can read one back or write one again.
--
-- Those rows go back to auto whole: the first installed CLI at spawn time, on
-- its own default model. The columns stay two — the launcher reads them apart,
-- and only the boundary spells them together.
UPDATE profiles       SET model = NULL WHERE agent_kind IS NULL AND model IS NOT NULL;
UPDATE goals          SET model = NULL WHERE agent_kind IS NULL AND model IS NOT NULL;
UPDATE tasks          SET model = NULL WHERE agent_kind IS NULL AND model IS NOT NULL;
UPDATE task_reviewers SET model = NULL WHERE agent_kind IS NULL AND model IS NOT NULL;
