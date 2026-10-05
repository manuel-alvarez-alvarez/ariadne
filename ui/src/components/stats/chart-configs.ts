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
  allowed: {
    label: "Allow",
    color: STATUS_COLORS.done,
    explain: "Allow: permission prompts this decider allowed.",
  },
  denied: {
    label: "Deny",
    color: STATUS_COLORS.danger,
    explain: "Deny: permission prompts this decider denied.",
  },
  cancelled: {
    label: "Cancelled",
    color: STATUS_COLORS.pending,
    explain: "Cancelled: permission prompts this decider cancelled.",
  },
} satisfies ChartConfig

export const attentionFlagConfig = {
  raised: {
    label: "Raised",
    color: STATUS_COLORS.active,
    explain: "Raised: attention flags raised for this reason.",
  },
} satisfies ChartConfig

/** The Work section's time chart: tasks per bucket, stacked by how they ended. */
export const WORK_CONFIG = {
  tasks_finished: {
    label: "Finished",
    color: STATUS_COLORS.done,
    explain: "Finished: tasks that reached finished in this bucket.",
  },
  tasks_failed: {
    label: "Failed",
    color: STATUS_COLORS.danger,
    explain: "Failed: tasks that reached failed in this bucket.",
  },
  tasks_cancelled: {
    label: "Cancelled",
    color: STATUS_COLORS.pending,
    explain: "Cancelled: tasks that reached cancelled in this bucket.",
  },
  goals_completed: {
    label: "Goals completed",
    explain: "Goals completed: the count of goals that reached completed in this bucket.",
  },
  landed: {
    label: "Landed",
    explain:
      "Landed: the count of finished tasks in this bucket whose landing was a merge or a pull request.",
  },
} satisfies ChartConfig

/** The total task time in each lifecycle status, and the two figures beside
 * it that have no bar of their own: the median and the status's share of
 * the whole. */
export const TIME_STATUS_CONFIG = {
  total_secs: {
    label: "Total",
    color: STATUS_COLORS.active,
    explain: "Total: the summed time tasks spent in this status.",
  },
  median_secs: {
    label: "Median",
    explain: "Median: the median time a task spent in this status.",
  },
  share: {
    label: "Share",
    explain: "Share: this status's share of the total time measured here.",
  },
} satisfies ChartConfig

/**
 * Input and output tokens, the two series the Spend section stacks: neither
 * is a status, so each draws in one of the ramp's two most neutral colours
 * rather than borrowing a meaning — done or danger — that belongs to an
 * outcome.
 */
export const SPEND_CONFIG: ChartConfig = {
  input_tokens: {
    label: "Input",
    color: STATUS_COLORS.active,
    explain: "Input: input tokens spent in this bucket, cache included.",
  },
  output_tokens: {
    label: "Output",
    color: STATUS_COLORS.done,
    explain: "Output: output tokens spent in this bucket.",
  },
  cached_share: {
    label: "Cached",
    explain: "Cached: the share of this bucket's input tokens that were served from cache.",
  },
}
