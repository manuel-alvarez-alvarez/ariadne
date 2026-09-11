import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { api, type ModelDto, qk, type SetModelEnabledRequest, unwrap } from "@/api"

/**
 * The catalog an agent can be pinned to: every model of every registry agent, with
 * the efforts each one takes and whether an agent can be staffed on it.
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

/**
 * Turns one model of the catalog on or off.
 *
 * The id rides in the body rather than the path because a model id carries
 * both `:` and, for the ids opencode discovers, `/` — which is a path of its
 * own, not a segment of one.
 *
 * The list is the only cache entry there is (the catalog has no detail key),
 * and the daemon answers with the entry as it now stands, so the row is
 * written into the list in place: a screen of toggles must not flicker back
 * to the old answer while a refetch is in flight.
 */
export function useSetModelEnabled() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: SetModelEnabledRequest) => unwrap(api().PUT("/v1/models/enabled", { body })),
    onSuccess: (updated) => {
      queryClient.setQueryData(qk.models.list(), (models: ModelDto[] | undefined) =>
        models?.map((model) => (model.id === updated.id ? updated : model)),
      )
      void queryClient.invalidateQueries({ queryKey: qk.models.lists() })
    },
  })
}
