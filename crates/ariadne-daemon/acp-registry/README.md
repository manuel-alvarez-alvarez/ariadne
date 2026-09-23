# The vendored ACP registry index

`registry.json` is a snapshot of the index the Agent Client Protocol
publishes, taken verbatim on **2026-09-23** from

    https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json

It is the map from an agent to the command that runs it: the daemon reads it
at every start, and registers each agent whose command is on its `PATH`.
Ariadne installs nothing, so a newer index adds agents a user can run, never
agents Ariadne downloads.

Refresh it by fetching that URL again and committing the file as it arrived.
