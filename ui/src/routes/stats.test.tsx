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

import { fireEvent, screen, within } from "@testing-library/react"
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

/**
 * Every dashboard the stub daemon knows, mutated by create and delete so a
 * list refetch after either — which the Stats screen's picker depends on to
 * show the right dashboard — reflects what was just done.
 */
let dashboards: DashboardDto[] = []

beforeEach(() => {
  requests = []
  dashboards = [STATS, CUSTOM]
  mockTheme.resolvedTheme = "light"
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    requests.push({ method: request.method, path: url.pathname })
    if (request.method === "GET" && url.pathname === "/v1/dashboards") {
      return jsonResponse(dashboards)
    }
    if (url.pathname === "/v1/dashboards/custom/reset") {
      return jsonResponse({ ...CUSTOM, document_is_default: true })
    }
    if (request.method === "POST" && url.pathname === "/v1/dashboards") {
      const body = (await request.json()) as { name: string; document: unknown }
      const created = aDashboard({ name: body.name, builtin: false, document: body.document })
      dashboards = [...dashboards, created]
      return jsonResponse(created, 201)
    }
    const deleteMatch = url.pathname.match(/^\/v1\/dashboards\/([^/]+)$/)
    if (request.method === "DELETE" && deleteMatch) {
      dashboards = dashboards.filter((one) => one.name !== deleteMatch[1])
      return new Response(null, { status: 204 })
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

  it("disables Rename and Delete on the built-in dashboard, and enables them on one of your own", async () => {
    const user = userEvent.setup()
    renderScreen(<StatsPage />, { route: "/stats" })
    await screen.findByTitle("stats dashboard")

    expect((screen.getByRole("button", { name: "Rename" }) as HTMLButtonElement).disabled).toBe(
      true,
    )
    expect((screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(
      true,
    )

    await user.click(screen.getByRole("combobox", { name: "Dashboard" }))
    await user.click(await screen.findByRole("option", { name: "custom" }))
    await screen.findByTitle("custom dashboard")

    expect((screen.getByRole("button", { name: "Rename" }) as HTMLButtonElement).disabled).toBe(
      false,
    )
    expect((screen.getByRole("button", { name: "Delete" }) as HTMLButtonElement).disabled).toBe(
      false,
    )
  })

  it("creates a dashboard and opens the picker on it", async () => {
    const user = userEvent.setup()
    renderScreen(<StatsPage />, { route: "/stats" })
    await screen.findByTitle("stats dashboard")

    await user.click(screen.getByRole("button", { name: "New dashboard" }))
    const createDialog = screen.getByRole("dialog", { name: "New dashboard" })
    await user.type(within(createDialog).getByLabelText("Name"), "on-call")
    await user.click(within(createDialog).getByRole("button", { name: "Create dashboard" }))

    expect(await screen.findByTitle("on-call dashboard")).toBeDefined()
  })

  it("renames a dashboard and opens the picker on the new name", async () => {
    const user = userEvent.setup()
    renderScreen(<StatsPage />, { route: "/stats?dashboard=custom" })
    await screen.findByTitle("custom dashboard")

    await user.click(screen.getByRole("button", { name: "Rename" }))
    const renameDialog = screen.getByRole("dialog", { name: "Rename custom" })
    fireEvent.change(within(renameDialog).getByLabelText("Name"), { target: { value: "" } })
    await user.type(within(renameDialog).getByLabelText("Name"), "on-call")
    await user.click(within(renameDialog).getByRole("button", { name: "Rename" }))

    expect(await screen.findByTitle("on-call dashboard")).toBeDefined()
    expect(requests).toContainEqual({ method: "DELETE", path: "/v1/dashboards/custom" })
  })

  it("asks before it deletes a dashboard, then opens the picker on a remaining one", async () => {
    const user = userEvent.setup()
    renderScreen(<StatsPage />, { route: "/stats?dashboard=custom" })
    await screen.findByTitle("custom dashboard")

    await user.click(screen.getByRole("button", { name: "Delete" }))
    const confirm = await screen.findByRole("dialog", { name: "Delete custom?" })
    await user.click(within(confirm).getByRole("button", { name: "Delete" }))

    expect(await screen.findByTitle("stats dashboard")).toBeDefined()
    expect(requests).toContainEqual({ method: "DELETE", path: "/v1/dashboards/custom" })
  })
})
