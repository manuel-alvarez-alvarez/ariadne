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
    color: STATUS_COLORS.active,
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

/**
 * How the Spend section's composition chart splits the span's tokens into
 * three slices that sum to the whole, cached input kept apart from the
 * fresh input beside it rather than folded back into one "input" slice: the
 * two share a colour family — `active` for the fresh tokens a session asked
 * for, `ready` for the ones a cache answered instead — while output keeps
 * the same `done` it draws in the trend chart above.
 */
export const SPEND_COMPOSITION_CONFIG: ChartConfig = {
  fresh_input_tokens: {
    label: "Fresh input",
    color: STATUS_COLORS.active,
    explain: "Fresh input: input tokens spent in this span that were not served from cache.",
  },
  cached_input_tokens: {
    label: "Cached input",
    color: STATUS_COLORS.ready,
    explain: "Cached input: input tokens spent in this span that were served from cache.",
  },
  output_tokens: {
    label: "Output",
    color: STATUS_COLORS.done,
    explain: "Output: output tokens spent across every session in this span.",
  },
}

/**
 * The six steps `time_stats` reports, in the Time section's share chart:
 * each a status a task's lifecycle passes through, in the same order the
 * family lists them (specs/023-stats.md rule 42), each on the one colour
 * the rest of the app already gives that step — the board's own Pending and
 * Ready, `active` for work under way, and a review's own three outcomes.
 */
export const TIME_STATUS_PIE_CONFIG: ChartConfig = {
  pending: {
    label: "Pending",
    color: STATUS_COLORS.pending,
    explain: "Pending: this status's share of the total time measured here.",
  },
  ready: {
    label: "Ready",
    color: STATUS_COLORS.ready,
    explain: "Ready: this status's share of the total time measured here.",
  },
  in_progress: {
    label: "In progress",
    color: STATUS_COLORS.active,
    explain: "In progress: this status's share of the total time measured here.",
  },
  under_review: {
    label: "Under review",
    color: STATUS_COLORS.review,
    explain: "Under review: this status's share of the total time measured here.",
  },
  changes_requested: {
    label: "Changes requested",
    color: STATUS_COLORS.warn,
    explain: "Changes requested: this status's share of the total time measured here.",
  },
  approved: {
    label: "Approved",
    color: STATUS_COLORS.approved,
    explain: "Approved: this status's share of the total time measured here.",
  },
}

/**
 * The Models section's ranked comparison charts: one bar per model, coloured
 * as a plain count since no model's figure is an outcome of its own. Agents
 * rank by tasks, orchestrators by goals; the rest of each row's figures ride
 * in the bar's tooltip and its `sr-only` table alone.
 */
export const MODELS_AGENT_CONFIG: ChartConfig = {
  tasks: {
    label: "Tasks",
    color: STATUS_COLORS.active,
    explain: "Tasks: the count of distinct tasks this model worked as this seat.",
  },
  tokens: {
    label: "Tokens",
    explain:
      "Tokens: input and output tokens this model used in this seat, cache not counted twice.",
  },
  time_secs: {
    label: "Time",
    explain: "Time: the summed session time this model ran in this seat.",
  },
  messages: {
    label: "Messages",
    explain: "Messages: the count of messages this model sent in this seat.",
  },
  tasks_finished: {
    label: "Finished",
    explain: "Finished: the count of this model's tasks that ended in the column it ran.",
  },
}

export const MODELS_ORCHESTRATOR_CONFIG: ChartConfig = {
  goals: {
    label: "Goals",
    color: STATUS_COLORS.active,
    explain: "Goals: the count of distinct goals this model orchestrated.",
  },
  tokens: {
    label: "Tokens",
    explain:
      "Tokens: input and output tokens this model used in this seat, cache not counted twice.",
  },
  time_secs: {
    label: "Time",
    explain: "Time: the summed session time this model ran in this seat.",
  },
  messages: {
    label: "Messages",
    explain: "Messages: the count of messages this model sent in this seat.",
  },
}
