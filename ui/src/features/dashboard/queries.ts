import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CreateDashboardRequest,
  cacheRow,
  type DashboardDto,
  dropRow,
  qk,
  unwrap,
} from "@/api"

import { copyDashboardDocument } from "./document"

/**
 * Every dashboard, shipped and saved. One unfiltered list — the daemon takes
 * no filters — read by the Dashboards screen's picker.
 */
export function dashboardsQueryOptions() {
  return queryOptions({
    queryKey: qk.dashboards.list(),
    queryFn: () => unwrap(api().GET("/v1/dashboards")),
  })
}

/** An empty dashboard, or a copy of another one, under a name of its own. */
export function useCreateDashboard() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateDashboardRequest): Promise<DashboardDto> =>
      unwrap(api().POST("/v1/dashboards", { body })),
    onSuccess: (dashboard) => cacheRow(queryClient, qk.dashboards, dashboard),
  })
}

/**
 * Deletes a dashboard of the user's own. The daemon refuses the built-in one
 * (409 `dashboard_builtin`) — it is reset rather than removed — so the
 * screen only offers this where `builtin` is false.
 */
export function useDeleteDashboard() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api().DELETE("/v1/dashboards/{name}", { params: { path: { name } } })),
    onSuccess: (_result, name) => dropRow(queryClient, qk.dashboards, name),
  })
}

/**
 * Renames a dashboard: there is no rename endpoint, the name being the
 * table's own key, so this creates the panels, layouts, variables and time
 * settings of `from` under `to`, then deletes `from` — in that order, so a
 * name already taken, or a validation error, leaves `from` exactly as it
 * was. The built-in dashboard is kept out of reach of this by the Dashboards
 * screen, the same as a delete: calling it there would create the copy and
 * then fail to delete `from`, the daemon refusing that half the same way it
 * refuses a plain delete of it.
 */
export function useRenameDashboard() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({
      from,
      to,
      document,
    }: {
      from: string
      to: string
      document: unknown
    }): Promise<DashboardDto> => {
      const created = await unwrap(
        api().POST("/v1/dashboards", {
          body: { name: to, document: copyDashboardDocument(document, to) },
        }),
      )
      await unwrap(api().DELETE("/v1/dashboards/{name}", { params: { path: { name: from } } }))
      return created
    },
    onSuccess: (dashboard, { from }) => {
      cacheRow(queryClient, qk.dashboards, dashboard)
      dropRow(queryClient, qk.dashboards, from)
    },
  })
}

/**
 * Puts a dashboard back on the document Ariadne ships, by dropping what was
 * saved over it. The Dashboards screen disables this while `document_is_default`.
 */
export function useResetDashboard() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string): Promise<DashboardDto> =>
      unwrap(api().POST("/v1/dashboards/{name}/reset", { params: { path: { name } } })),
    onSuccess: (dashboard) => cacheRow(queryClient, qk.dashboards, dashboard),
  })
}
