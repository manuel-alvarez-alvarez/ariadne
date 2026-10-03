/**
 * Every chart's own series config, in one file rather than one per panel, so
 * "failed is danger" and "a neutral count is active" are each written once
 * and every panel that draws that meaning imports the same constant — a
 * wrong meaning colour is then a diff here, not a colour that quietly drifts
 * from the one two panels down. `SESSIONS_BARS` lives here too: it is the one
 * bar grouping a chart's own correctness depends on, so it is pinned beside
 * the colours rather than inlined where a future edit could stack it back in.
 */

import type { ChartConfig } from "@/components/ui/chart"
import { STATUS_COLORS } from "./status-colors"

export const SESSIONS_CONFIG = {
  ended: { label: "Ended", color: STATUS_COLORS.done },
  failed: { label: "Failed", color: STATUS_COLORS.danger },
  stalled: { label: "Stalled", color: STATUS_COLORS.warn },
} satisfies ChartConfig

export const TOKENS_CONFIG = {
  tokens: { label: "Tokens", color: STATUS_COLORS.active },
} satisfies ChartConfig

/**
 * `stalled` stands beside the `ended`/`failed` stack rather than inside it:
 * it can overlap `failed` (a run that stalled and was then killed), and
 * stacking it would add a second count of the same session, drawing a bar
 * longer than `sessions`. `ended` and `failed` alone always sum to exactly
 * `sessions`, with no overlap between them.
 */
export const SESSIONS_BARS = [["ended", "failed"], "stalled"]

export const REVIEWS_CONFIG = {
  firstPassRate: { label: "First pass", color: STATUS_COLORS.active },
} satisfies ChartConfig

/**
 * `exhausted` is the one switch reason that is a warning; `automaticOther`
 * (an automatic switch for some other reason, which the daemon does not
 * raise today) and `other` (a switch asked by hand) carry no status of their
 * own, so both read as a neutral count rather than reusing `done` or
 * `pending` for a meaning that is not finishing or cancelling anything.
 */
export const SWITCHES_CONFIG = {
  exhausted: { label: "Exhausted", color: STATUS_COLORS.warn },
  automaticOther: { label: "Automatic", color: STATUS_COLORS.active },
  other: { label: "Other", color: STATUS_COLORS.active },
  arrivals: { label: "Arrivals", color: STATUS_COLORS.active },
} satisfies ChartConfig

export const TOOLS_CONFIG = {
  ok: { label: "Calls", color: STATUS_COLORS.active },
  errors: { label: "Errors", color: STATUS_COLORS.danger },
} satisfies ChartConfig

export const TOOL_MODELS_CONFIG = {
  calls: { label: "Calls", color: STATUS_COLORS.active },
} satisfies ChartConfig

export const PERMISSIONS_CONFIG = {
  allow: { label: "Allow", color: STATUS_COLORS.done },
  deny: { label: "Deny", color: STATUS_COLORS.danger },
  cancelled: { label: "Cancelled", color: STATUS_COLORS.pending },
} satisfies ChartConfig

export const ENDINGS_CONFIG = {
  finished: { label: "Finished", color: STATUS_COLORS.done },
  failed: { label: "Failed", color: STATUS_COLORS.danger },
  cancelled: { label: "Cancelled", color: STATUS_COLORS.pending },
} satisfies ChartConfig

export const LEAD_TIME_CONFIG = {
  median_lead_time_secs: { label: "Median lead time", color: STATUS_COLORS.active },
} satisfies ChartConfig
