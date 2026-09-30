-- The 2026-09-30 winner changes the question and its score scale, so each
-- stored pair must start again at the bounds selected for that contract.
UPDATE ai_permission_settings
SET allow_threshold = 0.0886,
    deny_threshold = 0.6256,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
