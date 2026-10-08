-- The commit a merged request landed as, as the forge reports it: what its
-- task is finished by (005). NULL until a read finds the request merged.
ALTER TABLE pull_requests ADD COLUMN merge_sha TEXT;
