-- Whether the user set the AI permission threshold pair by hand. A pair that
-- is not hand-set follows the default of the chosen flavour (022, rule 24),
-- so the stored pair is read only where this is 1. A row still at the pair
-- the init migration seeds was never set by hand; any other pair was.
ALTER TABLE ai_permission_settings
    ADD COLUMN thresholds_hand_set INTEGER NOT NULL DEFAULT 0
    CHECK (thresholds_hand_set IN (0, 1));
UPDATE ai_permission_settings
SET thresholds_hand_set = CASE
    WHEN allow_threshold = 0.0201 AND deny_threshold = 0.6321 THEN 0
    ELSE 1
END;
