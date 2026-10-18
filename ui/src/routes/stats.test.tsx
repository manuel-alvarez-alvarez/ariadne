// @vitest-environment jsdom

/**
 * The Stats screen against a stubbed daemon.
 *
 * The screen itself renders no chart: it opens the `ui/dashboard` package in
 * an iframe and leaves the dashboard to it. What is pinned here is the glue —
 * the picker over `GET /v1/dashboards`, the iframe's own query string
 * (`daemon`, `dashboard`, `theme`), the Reset action's confirm flow and its
 * `document_is_default` gate — not anything Perses draws.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { type DashboardDto, qk } from "@/api"
import { aDashboard } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { StatsPage } from "./stats"

/**
 * `next-themes` outside its own provider (which this harness never mounts)
 * answers a fixed, empty context, so a test that cares about a theme
 * *change* needs the hook itself under its control.
 */
const mockTheme = vi.hoisted(() => ({ resolvedTheme: "light" as string | undefined }))
vi.mock("next-themes", () => ({
  useTheme: () => ({ resolvedTheme: mockTheme.resolvedTheme, setTheme: vi.fn() }),
}))

const STATS: DashboardDto = aDashboard({ name: "stats", builtin: true, document_is_default: true })
const CUSTOM: DashboardDto = aDashboard({
  name: "custom",
  builtin: false,
  document_is_default: false,
  updated_at: "2026-01-02T00:00:00Z",
})

/** Every request the screen made, by path. */
let requests: { method: string; path: string }[] = []

beforeEach(() => {
  requests = []
  mockTheme.resolvedTheme = "light"
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    requests.push({ method: request.method, path: url.pathname })
    if (url.pathname === "/v1/dashboards") return jsonResponse([STATS, CUSTOM])
    if (url.pathname === "/v1/dashboards/custom/reset") {
      return jsonResponse({ ...CUSTOM, document_is_default: true })
    }
    return jsonResponse({})
  })
})

describe("StatsPage", () => {
  it("opens the iframe on the first dashboard, with the daemon, its name and the theme on its own query string", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const iframe = (await screen.findByTitle("stats dashboard")) as HTMLIFrameElement
    const src = new URL(iframe.getAttribute("src") ?? "", "http://localhost")
    expect(src.pathname).toBe("/dashboard/")
    expect(src.searchParams.get("daemon")).toBe("http://127.0.0.1:7676")
    expect(src.searchParams.get("dashboard")).toBe("stats")
    expect(src.searchParams.get("theme")).toBe("light")
  })

  it("opens the dashboard the URL names", async () => {
    renderScreen(<StatsPage />, { route: "/stats?dashboard=custom" })

    const iframe = (await screen.findByTitle("custom dashboard")) as HTMLIFrameElement
    const src = new URL(iframe.getAttribute("src") ?? "", "http://localhost")
    expect(src.searchParams.get("dashboard")).toBe("custom")
  })

  it("lists every dashboard in the picker, and switches the iframe on a pick", async () => {
    const user = userEvent.setup()
    renderScreen(<StatsPage />, { route: "/stats" })
    await screen.findByTitle("stats dashboard")

    await user.click(screen.getByRole("combobox", { name: "Dashboard" }))
    await user.click(await screen.findByRole("option", { name: "custom" }))

    expect(await screen.findByTitle("custom dashboard")).toBeDefined()
  })

  it("disables Reset on the shipped document, and enables it once something was saved over it", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })
    await screen.findByTitle("stats dashboard")
    expect((screen.getByRole("button", { name: "Reset" }) as HTMLButtonElement).disabled).toBe(true)

    const user = userEvent.setup()
    await user.click(screen.getByRole("combobox", { name: "Dashboard" }))
    await user.click(await screen.findByRole("option", { name: "custom" }))
    await screen.findByTitle("custom dashboard")

    expect((screen.getByRole("button", { name: "Reset" }) as HTMLButtonElement).disabled).toBe(
      false,
    )
  })

  it("asks before it resets a dashboard, then sends the reset", async () => {
    const user = userEvent.setup()
    renderScreen(<StatsPage />, { route: "/stats?dashboard=custom" })
    await screen.findByTitle("custom dashboard")

    await user.click(screen.getByRole("button", { name: "Reset" }))
    const dialog = await screen.findByRole("dialog", { name: "Reset custom?" })
    await user.click(within(dialog).getByRole("button", { name: "Reset" }))

    expect(requests).toContainEqual({ method: "POST", path: "/v1/dashboards/custom/reset" })
  })

  it("tells the iframe to load the document again once the shown dashboard is edited elsewhere", async () => {
    const { queryClient } = renderScreen(<StatsPage />, { route: "/stats?dashboard=custom" })
    const iframe = (await screen.findByTitle("custom dashboard")) as HTMLIFrameElement
    const post = vi.spyOn(iframe.contentWindow as Window, "postMessage")

    // The SSE dispatcher patches exactly this on a `dashboard_updated` event
    // (`src/events/dispatch.ts`); the screen reacts to the row changing, not
    // to the event itself.
    queryClient.setQueryData(qk.dashboards.list(), [
      STATS,
      { ...CUSTOM, updated_at: "2026-01-03T00:00:00Z" },
    ])

    await vi.waitFor(() => {
      expect(post).toHaveBeenCalledWith({ type: "reload" }, window.location.origin)
    })
  })

  it("keeps the iframe's src unchanged across a theme change, and posts the new theme instead", async () => {
    const { rerender } = renderScreen(<StatsPage />, { route: "/stats" })
    const iframe = (await screen.findByTitle("stats dashboard")) as HTMLIFrameElement
    const initialSrc = iframe.getAttribute("src")
    const post = vi.spyOn(iframe.contentWindow as Window, "postMessage")

    mockTheme.resolvedTheme = "dark"
    rerender(<StatsPage />)

    expect(iframe.getAttribute("src")).toBe(initialSrc)
    await vi.waitFor(() => {
      expect(post).toHaveBeenCalledWith({ type: "theme", theme: "dark" }, window.location.origin)
    })
  })
})
