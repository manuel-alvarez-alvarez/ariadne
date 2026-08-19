/**
 * The agents screen's read and its one write.
 *
 * `GET /v1/agents` answers with every agent kind at once — the daemon takes no
 * filters and there is no per-kind endpoint to read — so there is a single
 * list key here and no detail keys under it.
 *
 * Nothing on the stream carries an agent config: there is no
 * `agent_config_updated` event, so unlike profiles or repositories this list
 * is only ever moved by the write below. That makes patching the list in
 * `onSuccess` the point rather than a nicety — the `PUT` answers with the
 * whole updated config, so the row is correct before the refetch lands.
 */

import type { QueryClient } from "@tanstack/react-query"
import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { type AgentConfigDto, type AgentKind, api, qk, unwrap } from "@/api"

/** `GET /v1/agents` — every agent kind's flags, current and default. */
export function agentConfigsQueryOptions() {
  return queryOptions({
    queryKey: qk.agents.list(),
    queryFn: () => unwrap(api().GET("/v1/agents")),
  })
}

/**
 * `PUT /v1/agents/{kind}` — the whole flag list, empty included.
 *
 * There is no adding to the list and no clearing sentinel: what is sent is
 * what the agent is launched with, and restoring the defaults is this same
 * call with the kind's `default_flags`.
 */
export function useUpdateAgentConfig() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ kind, extraFlags }: { kind: AgentKind; extraFlags: string[] }) =>
      unwrap(
        api().PUT("/v1/agents/{kind}", {
          params: { path: { kind } },
          body: { extra_flags: extraFlags },
        }),
      ),
    onSuccess: (config) => cacheAgentConfig(queryClient, config),
  })
}

/** The updated kind back into the list, in place, and a refetch behind it. */
function cacheAgentConfig(queryClient: QueryClient, config: AgentConfigDto): void {
  queryClient.setQueryData(qk.agents.list(), (configs?: AgentConfigDto[]) =>
    configs?.map((current) => (current.agent_kind === config.agent_kind ? config : current)),
  )
  void queryClient.invalidateQueries({ queryKey: qk.agents.lists() })
}
