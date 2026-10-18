import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { api, cacheRow, type DashboardDto, qk, unwrap } from "@/api"

/**
 * Every dashboard, shipped and saved. One unfiltered list — the daemon takes
 * no filters — read by the Stats screen's picker.
 */
export function dashboardsQueryOptions() {
  return queryOptions({
    queryKey: qk.dashboards.list(),
    queryFn: () => unwrap(api().GET("/v1/dashboards")),
  })
}

/**
 * Puts a dashboard back on the document Ariadne ships, by dropping what was
 * saved over it. The Stats screen disables this while `document_is_default`.
 */
export function useResetDashboard() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string): Promise<DashboardDto> =>
      unwrap(api().POST("/v1/dashboards/{name}/reset", { params: { path: { name } } })),
    onSuccess: (dashboard) => cacheRow(queryClient, qk.dashboards, dashboard),
  })
}
