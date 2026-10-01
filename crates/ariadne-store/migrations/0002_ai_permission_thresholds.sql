-- Apply the reviewed threshold pair to fresh and existing settings.
UPDATE ai_permission_settings
SET allow_threshold = 0.0531, deny_threshold = 0.6522
WHERE id = 1;
