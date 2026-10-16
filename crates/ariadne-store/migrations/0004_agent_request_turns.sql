-- An agent request belongs to the prompt turn that last raised it. The ACP
-- runtime keeps the active turn in memory while its MCP call can name it.
ALTER TABLE agent_requests ADD COLUMN last_raised_turn_id TEXT;
