-- The flavour and device the user chose (022, flavours and devices). A NULL
-- device is filled by the daemon at startup with the best device that runs
-- the stored flavour, so an install from before flavours existed keeps
-- `4b` and gets a device without a fresh choice.
ALTER TABLE ai_permission_settings
ADD COLUMN flavour TEXT NOT NULL DEFAULT '4b' CHECK (flavour IN ('0.8b', '4b', '9b', '27b'));
ALTER TABLE ai_permission_settings
ADD COLUMN device TEXT CHECK (device IN ('mlx', 'cuda', 'cpu'));

-- The scheduled refresh is gone; refresh is manual only.
ALTER TABLE ai_permission_settings DROP COLUMN schedule;
ALTER TABLE ai_permission_settings DROP COLUMN last_scheduled_refresh;
