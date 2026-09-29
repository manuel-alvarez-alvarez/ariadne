UPDATE ai_permission_settings
SET allow_threshold = 0.1647,
    deny_threshold = 0.626,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE allow_threshold = 0.1338
  AND deny_threshold = 0.5345;
