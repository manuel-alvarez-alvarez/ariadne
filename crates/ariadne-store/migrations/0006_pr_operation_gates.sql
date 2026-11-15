-- Upgrade custom operation workflows without replacing their other settings.
UPDATE workflows
SET document = rtrim(document) || char(10) || '    gate: request-merged' || char(10)
WHERE name IN ('pr-review', 'pr-babysit') AND document IS NOT NULL;

-- Keep completed history unchanged.
UPDATE goal_steps SET gate = 'request-merged'
WHERE goal_id IN (
    SELECT id FROM goals WHERE pr_operation IN ('review', 'babysit')
    AND status IN ('planning', 'active')
);
