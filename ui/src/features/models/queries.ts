import { queryOptions } from "@tanstack/react-query"

import { api, qk, unwrap } from "@/api"

/**
 * The catalog an agent can be pinned to: every model of every agent CLI, with
 * the efforts each one takes.
 *
 * One unfiltered list — the picker narrows it client-side, and the daemon
 * takes no filters — so it is keyed once and shared by every screen that
 * chooses a model.
 */
export function modelsQueryOptions() {
  return queryOptions({
    queryKey: qk.models.list(),
    queryFn: () => unwrap(api().GET("/v1/models")),
  })
}
