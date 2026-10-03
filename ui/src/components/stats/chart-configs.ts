/**
 * Every chart's own series config, in one file rather than one per section,
 * so "failed is danger" and "a neutral count is active" are each written once
 * and every section that draws that meaning imports the same constant — a
 * wrong meaning colour is then a diff here, not a colour that quietly drifts
 * from the one two sections down. Each family adds the configs its charts
 * draw, built off `STATUS_COLORS`.
 */

import type { ChartConfig } from "@/components/ui/chart"
import { STATUS_COLORS } from "./status-colors"

export const attentionPermissionConfig = {
  allowed: { label: "Allow", color: STATUS_COLORS.done },
  denied: { label: "Deny", color: STATUS_COLORS.danger },
  cancelled: { label: "Cancelled", color: STATUS_COLORS.pending },
} satisfies ChartConfig

export const attentionFlagConfig = {
  raised: { label: "Raised", color: STATUS_COLORS.active },
} satisfies ChartConfig

/** The Work section's time chart: tasks per bucket, stacked by how they ended. */
export const WORK_CONFIG = {
  tasks_finished: { label: "Finished", color: STATUS_COLORS.done },
  tasks_failed: { label: "Failed", color: STATUS_COLORS.danger },
  tasks_cancelled: { label: "Cancelled", color: STATUS_COLORS.pending },
  goals_completed: { label: "Goals completed" },
  landed: { label: "Landed" },
} satisfies ChartConfig
