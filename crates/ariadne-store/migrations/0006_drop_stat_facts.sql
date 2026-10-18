-- Metrics now live in telemetry.db.  The retired ledger contained derived
-- aggregates only, so no operational state is lost by removing it.
DROP TABLE stat_facts;
