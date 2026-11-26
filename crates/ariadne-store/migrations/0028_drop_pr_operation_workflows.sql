-- A pull request review or babysit runs on one column that Ariadne fixes in
-- code, so the catalog keeps no workflow for either operation. Drop both
-- rows, edited or not: an edited one would otherwise stay as a workflow of
-- the user's own. Open operation goals keep the columns they snapshotted.
DELETE FROM workflows WHERE name IN ('pr-review', 'pr-babysit');
