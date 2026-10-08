-- Why the last fetch of a repository's requests failed, or NULL once one
-- succeeds (026): polling has no hook to report on, so this is what says it
-- works.
ALTER TABLE forge_integrations ADD COLUMN fetch_error TEXT;
