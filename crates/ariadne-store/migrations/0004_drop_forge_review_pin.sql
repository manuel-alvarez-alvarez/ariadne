-- Every review now starts manually, through the ask route. The repository's
-- own review pin no longer starts one on its own, so it is dropped; the
-- per-request pin on `pull_requests` stays.
ALTER TABLE forge_integrations DROP COLUMN review_model;
ALTER TABLE forge_integrations DROP COLUMN review_effort;
