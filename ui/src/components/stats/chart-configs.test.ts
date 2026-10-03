import { describe, expect, it } from "vitest"

import {
  ENDINGS_CONFIG,
  LEAD_TIME_CONFIG,
  PERMISSIONS_CONFIG,
  REVIEWS_CONFIG,
  SESSIONS_BARS,
  SESSIONS_CONFIG,
  SWITCHES_CONFIG,
  TOKENS_CONFIG,
  TOOL_MODELS_CONFIG,
  TOOLS_CONFIG,
} from "./chart-configs"
import { STATUS_COLORS } from "./status-colors"

/**
 * Every chart's own series config, pinned key by key — so a wrong meaning
 * colour (a failed series painted `warn`, a cancelled one painted `done`)
 * fails here rather than only looking off on screen.
 */
describe("the chart series configs", () => {
  it("colours the models panel's sessions by how they ended, and its tokens as a neutral count", () => {
    expect(SESSIONS_CONFIG).toEqual({
      ended: { label: "Ended", color: STATUS_COLORS.done },
      failed: { label: "Failed", color: STATUS_COLORS.danger },
      stalled: { label: "Stalled", color: STATUS_COLORS.warn },
    })
    expect(TOKENS_CONFIG).toEqual({
      tokens: { label: "Tokens", color: STATUS_COLORS.active },
    })
  })

  it("colours the reviews panel's first-pass rate as a neutral count", () => {
    expect(REVIEWS_CONFIG).toEqual({
      firstPassRate: { label: "First pass", color: STATUS_COLORS.active },
    })
  })

  it("colours the switches panel's exhaustion as a warning and every other reason as a neutral count", () => {
    expect(SWITCHES_CONFIG).toEqual({
      exhausted: { label: "Exhausted", color: STATUS_COLORS.warn },
      automaticOther: { label: "Automatic", color: STATUS_COLORS.active },
      other: { label: "Other", color: STATUS_COLORS.active },
      arrivals: { label: "Arrivals", color: STATUS_COLORS.active },
    })
  })

  it("colours the tools panel's calls, errors and permission answers", () => {
    expect(TOOLS_CONFIG).toEqual({
      ok: { label: "Calls", color: STATUS_COLORS.active },
      errors: { label: "Errors", color: STATUS_COLORS.danger },
    })
    expect(TOOL_MODELS_CONFIG).toEqual({
      calls: { label: "Calls", color: STATUS_COLORS.active },
    })
    expect(PERMISSIONS_CONFIG).toEqual({
      allow: { label: "Allow", color: STATUS_COLORS.done },
      deny: { label: "Deny", color: STATUS_COLORS.danger },
      cancelled: { label: "Cancelled", color: STATUS_COLORS.pending },
    })
  })

  it("colours the outcomes panel's endings and its lead time", () => {
    expect(ENDINGS_CONFIG).toEqual({
      finished: { label: "Finished", color: STATUS_COLORS.done },
      failed: { label: "Failed", color: STATUS_COLORS.danger },
      cancelled: { label: "Cancelled", color: STATUS_COLORS.pending },
    })
    expect(LEAD_TIME_CONFIG).toEqual({
      median_lead_time_secs: { label: "Median lead time", color: STATUS_COLORS.active },
    })
  })

  it("keeps stalled out of the ended/failed stack, since it can overlap failed and stacking it would draw a bar longer than sessions", () => {
    expect(SESSIONS_BARS).toEqual([["ended", "failed"], "stalled"])
  })

  it("gives one meaning the same colour in every panel that carries it", () => {
    expect(TOOLS_CONFIG.errors.color).toBe(SESSIONS_CONFIG.failed.color)
    expect(ENDINGS_CONFIG.failed.color).toBe(SESSIONS_CONFIG.failed.color)
    expect(PERMISSIONS_CONFIG.deny.color).toBe(SESSIONS_CONFIG.failed.color)
    expect(PERMISSIONS_CONFIG.cancelled.color).toBe(ENDINGS_CONFIG.cancelled.color)
    expect(PERMISSIONS_CONFIG.allow.color).toBe(SESSIONS_CONFIG.ended.color)
    expect(ENDINGS_CONFIG.finished.color).toBe(SESSIONS_CONFIG.ended.color)
    expect(REVIEWS_CONFIG.firstPassRate.color).toBe(TOKENS_CONFIG.tokens.color)
    expect(TOOL_MODELS_CONFIG.calls.color).toBe(TOKENS_CONFIG.tokens.color)
    expect(TOOLS_CONFIG.ok.color).toBe(TOKENS_CONFIG.tokens.color)
    expect(SWITCHES_CONFIG.arrivals.color).toBe(TOKENS_CONFIG.tokens.color)
    expect(LEAD_TIME_CONFIG.median_lead_time_secs.color).toBe(TOKENS_CONFIG.tokens.color)
  })
})
