/**
 * The agents screen's read and its one write.
 *
 * `GET /v1/agents` answers with every registry agent at once — the daemon
 * takes no filters and there is no per-agent endpoint to read — so there is a
 * single list key here and no detail keys under it.
 *
 * Nothing on the stream carries an agent config: there is no
 * `agent_config_updated` event, so unlike profiles or repositories this list
 * is only ever moved by the write below. That makes patching the list in
 * `onSuccess` the point rather than a nicety — the `PUT` answers with the
 * whole updated config, so the row is correct before the refetch lands.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"
import { type AgentConfigDto, api, qk, unwrap } from "@/api"

/** `GET /v1/agents` — every registry agent's flags, current and default. */
export function agentConfigsQueryOptions() {
  return queryOptions({
    queryKey: qk.agents.list(),
    queryFn: () => unwrap(api().GET("/v1/agents")),
  })
}

/**
 * `GET /v1/acp-agents` — every built-in and configured ACP agent with its
 * cached discovery result: whether it is `ready` or `rejected`, and — the
 * `session_list` capability among them — whether it can list its own stored
 * sessions for adoption.
 */
export function acpAgentsQueryOptions() {
  return queryOptions({
    queryKey: qk.acpAgents.list(),
    queryFn: () => unwrap(api().GET("/v1/acp-agents")),
  })
}

/**
 * `PUT /v1/agents/{id}` — the whole flag list, empty included.
 *
 * There is no adding to the list and no clearing sentinel: what is sent is
 * what the agent is launched with, and restoring the defaults is this same
 * call with the agent's `default_flags`.
 */
export function useUpdateAgentConfig() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ agentId, extraFlags }: { agentId: string; extraFlags: string[] }) =>
      unwrap(
        api().PUT("/v1/agents/{id}", {
          params: { path: { id: agentId } },
          body: { extra_flags: extraFlags },
        }),
      ),
    // The updated agent back into the list, in place, and a refetch behind it.
    // There are no detail keys to patch, so this is not `cacheRow`.
    onSuccess: (config) => {
      queryClient.setQueryData(qk.agents.list(), (configs?: AgentConfigDto[]) =>
        configs?.map((current) => (current.agent_id === config.agent_id ? config : current)),
      )
      void queryClient.invalidateQueries({ queryKey: qk.agents.lists() })
    },
  })
}
