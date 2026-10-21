// @vitest-environment node

/**
 * The shipped document is store-owned, but this package is the Perses runtime
 * that opens it. Exercise the installed Perses layout conversion directly:
 * mounting all of ViewDashboard in jsdom is its integration suite, not ours.
 */

import type { DashboardResource } from "@perses-dev/client"
import { convertLayoutsToPanelGroups } from "@perses-dev/dashboards/dist/context/DashboardProvider/panel-group-slice"
import { describe, expect, it } from "vitest"

import defaultDashboard from "../../../crates/ariadne-store/dashboards/default.json"

// The daemon's stored document has no project metadata. `page.tsx` adds that
// Perses-only field before rendering it, so this test needs only its spec.
const shippedDefault = defaultDashboard as unknown as DashboardResource

// The plugin spec of a panel is `UnknownSpec` to Perses's own types, so a
// legend check reaches through a cast, the same way `shippedDefault` itself does.
function legendOf(panel: string): { mode?: string; values?: string[] } | undefined {
  const spec = shippedDefault.spec.panels[panel]?.spec.plugin.spec as Record<string, unknown>
  return spec.legend as { mode?: string; values?: string[] } | undefined
}

describe("the shipped default dashboard", () => {
  it("shows every service and level in its operational log table", () => {
    const panel = shippedDefault.spec.panels["warnings-and-errors"]
    const query = panel?.spec.queries?.[0]?.spec.plugin.spec as { query: string } | undefined
    expect(query?.query).toBe('{service=~".+"}')
  })
  it("uses Default as its display name while retaining default as its metadata name", () => {
    expect(shippedDefault.metadata.name).toBe("default")
    expect(shippedDefault.spec.display?.name).toBe("Default")
  })

  it("opens through the installed Perses layout converter", () => {
    expect(() => convertLayoutsToPanelGroups(shippedDefault.spec.layouts)).not.toThrow()
  })

  it("lays out every panel once, and refers to no panel it lacks", () => {
    const laidOut = shippedDefault.spec.layouts.flatMap((layout) =>
      "items" in layout.spec
        ? layout.spec.items.map((item) => item.content.$ref.replace("#/spec/panels/", ""))
        : [],
    )
    expect([...laidOut].sort()).toEqual(Object.keys(shippedDefault.spec.panels).sort())
  })

  it("names only the variables it declares or Perses builds in", () => {
    const declared = new Set(shippedDefault.spec.variables.map((variable) => variable.spec.name))
    const builtin = new Set(["__range", "__rate_interval", "__interval"])
    const used = JSON.stringify(shippedDefault.spec.panels).matchAll(/\$\{?(\w+)/g)
    const unknown = [...used]
      .map((match) => match[1] ?? "")
      .filter((name) => !declared.has(name) && !builtin.has(name))
    expect(unknown).toEqual([])
  })

  const PIE_CHARTS = [
    "task-outcomes",
    "goal-outcomes",
    "tokens-share",
    "tokens-by-seat",
    "runs-by-seat",
    "permissions-by-decider",
    "permissions-by-answer",
  ]

  it.each(PIE_CHARTS)(
    "%s shows a table legend with the value and percent of each slice",
    (panel) => {
      expect(legendOf(panel)).toMatchObject({ mode: "table", values: ["abs", "relative"] })
    },
  )

  const SUM_OVER_TIME = [
    "tasks-over-time",
    "tokens-over-time",
    "turns-over-time",
    "attention-over-time",
  ]

  it.each(SUM_OVER_TIME)("%s shows a table legend with the sum of each series", (panel) => {
    expect(legendOf(panel)).toMatchObject({ mode: "table", values: ["sum"] })
  })

  it("lead-time-trend shows a table legend with the mean and the max of each series", () => {
    expect(legendOf("lead-time-trend")).toMatchObject({ mode: "table", values: ["mean", "max"] })
  })

  const DAEMON_HEALTH = [
    "http-latency",
    "http-requests",
    "scheduler-tick",
    "launches",
    "db-size",
    "memory",
    "cpu",
    "stream-clients",
  ]

  it.each(DAEMON_HEALTH)("%s keeps a plain list legend with no values", (panel) => {
    expect(legendOf(panel)).toMatchObject({ mode: "list" })
    expect(legendOf(panel)?.values).toBeUndefined()
  })
})
