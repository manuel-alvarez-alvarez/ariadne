// @vitest-environment node

/**
 * The shipped document is store-owned, but this package is the Perses runtime
 * that opens it. Exercise the installed Perses layout conversion directly:
 * mounting all of ViewDashboard in jsdom is its integration suite, not ours.
 */

import type { DashboardResource } from "@perses-dev/client"
import { convertLayoutsToPanelGroups } from "@perses-dev/dashboards/dist/context/DashboardProvider/panel-group-slice"
import { describe, expect, it } from "vitest"

import stats from "../../../crates/ariadne-store/dashboards/stats.json"

// The daemon's stored document has no project metadata. `page.tsx` adds that
// Perses-only field before rendering it, so this test needs only its spec.
const shippedStats = stats as unknown as DashboardResource

describe("the shipped stats dashboard", () => {
  it("opens through the installed Perses layout converter", () => {
    expect(() => convertLayoutsToPanelGroups(shippedStats.spec.layouts)).not.toThrow()
  })
})
