-- The description of a request, as the forge holds it: what the desktop's
-- filter searches beside the title. Empty on every row until its next read.
ALTER TABLE pull_requests ADD COLUMN body TEXT NOT NULL DEFAULT '';
