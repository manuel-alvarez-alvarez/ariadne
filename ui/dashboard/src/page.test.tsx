// @vitest-environment jsdom

/**
 * `Page`'s own lifecycle, with Perses's heavy pieces stubbed out: real
 * `ViewDashboard` is too heavy for jsdom (rule 5), so this mocks it with a
 * component that only records how many times it mounted and what document it
 * was given — enough to prove the two claims below without ever drawing a
 * real chart.
 *
 * - React's StrictMode mounts, unmounts and remounts a component once in
 *   development to catch an effect that cleans up wrong. `mounted.current`
 *   in `page.tsx` has to come back up on that remount, or every `load()`
 *   after it silently drops its own result.
 * - A `{type: "reload"}` message has to hand `ViewDashboard` a document it
 *   reads as *new*, not a prop update into the same instance: it keeps its
 *   own internal store from its first `dashboardResource`, and a prop change
 *   alone never reaches it.
 */

import { act, render, screen } from "@testing-library/react"
import { StrictMode, useEffect } from "react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { daemonFetch } from "./test/setup"

let mounts = 0
let lastSpec = ""
let lastIsEditing: boolean | undefined

// `vi.mock` hoists above every import in the file, regardless of where it is
// written, so `Page` below reads these in place of the real modules.
vi.mock("@perses-dev/dashboards", () => ({
  ViewDashboard: (props: { dashboardResource: { spec: unknown }; isEditing?: boolean }) => {
    // `mounts` counts actual mounts, through an effect with no dependency —
    // not renders, which a prop update alone would also produce without
    // proving the component (and so Perses's own internal store) was torn
    // down and recreated.
    useEffect(() => {
      mounts += 1
    }, [])
    lastSpec = JSON.stringify(props.dashboardResource.spec)
    lastIsEditing = props.isEditing
    return <div data-testid="view-dashboard">{lastSpec}</div>
  },
}))

vi.mock("./plugins", () => ({
  pluginLoader: {
    getInstalledPlugins: async () => [],
    importPluginModule: async () => ({}),
  },
}))

import { Page } from "./page"

function document(spec: Record<string, unknown>): unknown {
  return { kind: "Dashboard", metadata: { name: "stats" }, spec }
}

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
  })
}

beforeEach(() => {
  mounts = 0
  lastSpec = ""
  lastIsEditing = undefined
  window.history.pushState({}, "", "/?daemon=http://127.0.0.1:7676&dashboard=stats&theme=light")
})

describe("Page", () => {
  it("opens the dashboard in view mode, not Perses's own editing mode", async () => {
    daemonFetch.mockImplementation(async () => jsonResponse({ document: document({}) }))

    render(<Page />)

    await screen.findByTestId("view-dashboard")
    expect(lastIsEditing).toBeFalsy()
  })

  it("still shows the loaded dashboard under StrictMode's development double-invoke", async () => {
    daemonFetch.mockImplementation(async () => jsonResponse({ document: document({}) }))

    render(
      <StrictMode>
        <Page />
      </StrictMode>,
    )

    expect((await screen.findByTestId("view-dashboard")).textContent).toBe("{}")
  })

  it("remounts ViewDashboard with the fresh document on a reload message", async () => {
    let call = 0
    daemonFetch.mockImplementation(async () => {
      call += 1
      return jsonResponse({ document: document(call === 1 ? {} : { changed: true }) })
    })

    render(<Page />)
    await screen.findByTestId("view-dashboard")
    const mountsAfterOpen = mounts

    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", { data: { type: "reload" }, origin: window.location.origin }),
      )
    })

    await vi.waitFor(() => {
      expect(mounts).toBeGreaterThan(mountsAfterOpen)
    })
    expect(lastSpec).toBe('{"changed":true}')
    expect(lastIsEditing).toBeFalsy()
  })
})
