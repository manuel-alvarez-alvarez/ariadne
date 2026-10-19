import type { DashboardDto } from "@/api"

/** The name Perses displays, or the dashboard's stable metadata name. */
export function dashboardDisplayName(dashboard: DashboardDto): string {
  const display = (dashboard.document as { spec?: { display?: { name?: unknown } } })?.spec?.display
  return typeof display?.name === "string" ? display.name : dashboard.name
}
