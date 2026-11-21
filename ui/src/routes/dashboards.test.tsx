// @vitest-environment jsdom

/**
 * The Dashboards screen against a stubbed daemon.
 *
 * The screen itself renders no chart: it opens the `ui/dashboard` package in
 * an iframe and leaves the dashboard to it. What is pinned here is the glue —
 * the picker over `GET /v1/dashboards`, the iframe's own query string
 * (`daemon`, `dashboard`, `theme`), the Reset action's confirm flow and its
 * `document_is_default` gate — not anything Perses draws.
 */

import { act, fireEvent, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { type DashboardDto, type HostDto, qk } from "@/api"
import { useSettingsStore } from "@/stores/settings"
import { aDashboard } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL } from "@/test/hosts"
import { DashboardsPage } from "./dashboards"

/**
 * `next-themes` outside its own provider (which this harness never mounts)
 * answers a fixed, empty context, so a test that cares about a theme
 * *change* needs the hook itself under its control.
 */
const mockTheme = vi.hoisted(() => ({ resolvedTheme: "light" as string | undefined }))
vi.mock("next-themes", () => ({
  useTheme: () => ({ resolvedTheme: mockTheme.resolvedTheme, setTheme: vi.fn() }),
}))

const DEFAULT_DASHBOARD: DashboardDto = aDashboard({
  name: "default",
  builtin: true,
  document_is_default: true,
  document: {
    kind: "Dashboard",
    metadata: { name: "default" },
    spec: { display: { name: "Default" } },
  },
})
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
 * list refetch after either — which the Dashboards screen's picker depends on to
 * show the right dashboard — reflects what was just done.
 */
let dashboards: DashboardDto[] = []

/** What `GET /v1/hosts` answers; empty reads as `local` alone. */
let hostRows: HostDto[] = []

afterEach(() => {
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

beforeEach(() => {
  requests = []
  hostRows = []
  // `custom` sorts before `default` — the daemon lists dashboards by name —
  // so a test that opens with no `?dashboard=` and still lands on `default`
  // is proof the screen picks the shipped dashboard by name, not by position.
  dashboards = [CUSTOM, DEFAULT_DASHBOARD]
  mockTheme.resolvedTheme = "light"
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    requests.push({ method: request.method, path: url.pathname })
    if (url.pathname === "/v1/hosts") return jsonResponse(hostRows)
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

describe("DashboardsPage", () => {
  it("opens the iframe on the shipped default dashboard when the URL names none, even though custom sorts before it, with the daemon, its name and the theme on its own query string", async () => {
    renderScreen(<DashboardsPage />, { route: "/dashboards" })

    const iframe = (await screen.findByTitle("Default dashboard")) as HTMLIFrameElement
    const src = new URL(iframe.getAttribute("src") ?? "", "http://localhost")
    expect(src.pathname).toBe("/dashboard/")
    expect(src.searchParams.get("daemon")).toBe("http://127.0.0.1:7676")
    expect(src.searchParams.get("dashboard")).toBe("default")
    expect(src.searchParams.get("theme")).toBe("light")
  })

  it("falls back to the first dashboard the list answers with where none is named default", async () => {
    dashboards = [CUSTOM]
    renderScreen(<DashboardsPage />, { route: "/dashboards" })

    expect(await screen.findByTitle("custom dashboard")).toBeDefined()
  })

  it("treats an empty dashboard parameter as naming none, and opens the default dashboard", async () => {
    renderScreen(<DashboardsPage />, { route: "/dashboards?dashboard=" })

    expect(await screen.findByTitle("Default dashboard")).toBeDefined()
  })

  it("opens the dashboard the URL names", async () => {
    renderScreen(<DashboardsPage />, { route: "/dashboards?dashboard=custom" })

    const iframe = (await screen.findByTitle("custom dashboard")) as HTMLIFrameElement
    const src = new URL(iframe.getAttribute("src") ?? "", "http://localhost")
    expect(src.searchParams.get("dashboard")).toBe("custom")
  })

  it("lists display names in the picker, falls back to metadata names, and keeps the id in the iframe", async () => {
    const user = userEvent.setup()
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    await screen.findByTitle("Default dashboard")

    await user.click(screen.getByRole("combobox", { name: "Dashboard" }))
    expect(await screen.findByRole("option", { name: "Default" })).toBeDefined()
    await user.click(await screen.findByRole("option", { name: "custom" }))

    expect(await screen.findByTitle("custom dashboard")).toBeDefined()
  })

  it("disables Reset on the shipped document, and enables it once something was saved over it", async () => {
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    await screen.findByTitle("Default dashboard")
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
    renderScreen(<DashboardsPage />, { route: "/dashboards?dashboard=custom" })
    await screen.findByTitle("custom dashboard")

    await user.click(screen.getByRole("button", { name: "Reset" }))
    const dialog = await screen.findByRole("dialog", { name: "Reset custom?" })
    await user.click(within(dialog).getByRole("button", { name: "Reset" }))

    expect(requests).toContainEqual({ method: "POST", path: "/v1/dashboards/custom/reset" })
  })

  it("uses the selected dashboard's display name in Reset and Delete confirmations", async () => {
    dashboards = [
      DEFAULT_DASHBOARD,
      {
        ...CUSTOM,
        document: {
          kind: "Dashboard",
          metadata: { name: "custom" },
          spec: { display: { name: "On call" } },
        },
      },
    ]
    const user = userEvent.setup()
    renderScreen(<DashboardsPage />, { route: "/dashboards?dashboard=custom" })
    await screen.findByTitle("On call dashboard")

    await user.click(screen.getByRole("button", { name: "Reset" }))
    expect(await screen.findByRole("dialog", { name: "Reset On call?" })).toBeDefined()
    await user.click(screen.getByRole("button", { name: "Cancel" }))

    await user.click(screen.getByRole("button", { name: "Delete" }))
    expect(await screen.findByRole("dialog", { name: "Delete On call?" })).toBeDefined()
  })

  it("tells the iframe to load the document again once the shown dashboard is edited elsewhere", async () => {
    const { queryClient } = renderScreen(<DashboardsPage />, {
      route: "/dashboards?dashboard=custom",
    })
    const iframe = (await screen.findByTitle("custom dashboard")) as HTMLIFrameElement
    const post = vi.spyOn(iframe.contentWindow as Window, "postMessage")

    // The SSE dispatcher patches exactly this on a `dashboard_updated` event
    // (`src/events/dispatch.ts`); the screen reacts to the row changing, not
    // to the event itself.
    queryClient.setQueryData(qk.dashboards.list("local"), [
      DEFAULT_DASHBOARD,
      { ...CUSTOM, updated_at: "2026-01-03T00:00:00Z" },
    ])

    await vi.waitFor(() => {
      expect(post).toHaveBeenCalledWith({ type: "reload" }, window.location.origin)
    })
  })

  it("keeps the iframe's src unchanged across a theme change, and posts the new theme instead", async () => {
    const { rerender } = renderScreen(<DashboardsPage />, { route: "/dashboards" })
    const iframe = (await screen.findByTitle("Default dashboard")) as HTMLIFrameElement
    const initialSrc = iframe.getAttribute("src")
    const post = vi.spyOn(iframe.contentWindow as Window, "postMessage")

    mockTheme.resolvedTheme = "dark"
    rerender(<DashboardsPage />)

    expect(iframe.getAttribute("src")).toBe(initialSrc)
    await vi.waitFor(() => {
      expect(post).toHaveBeenCalledWith({ type: "theme", theme: "dark" }, window.location.origin)
    })
  })

  it("queries every host under All, the picked host otherwise, and the host the select names", async () => {
    const user = userEvent.setup()
    hostRows = [LOCAL, BOX]
    const hostOf = (iframe: HTMLElement) =>
      new URL(iframe.getAttribute("src") ?? "", "http://localhost").searchParams.get("host")

    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    expect(hostOf(await screen.findByTitle("Default dashboard"))).toBe("all")

    await user.click(screen.getByRole("combobox", { name: "Host" }))
    const options = (await screen.findAllByRole("option")).map((option) => option.textContent)
    expect(options).toEqual(["All hosts", "local", "box"])
    await user.click(screen.getByRole("option", { name: "box" }))
    expect(hostOf(await screen.findByTitle("Default dashboard"))).toBe("box")
  })

  it("draws each host of the host select with its colour dot", async () => {
    const user = userEvent.setup()
    hostRows = [LOCAL, { ...BOX, color: "teal" }]
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    await user.click(await screen.findByRole("combobox", { name: "Host" }))
    const hues = (await screen.findAllByRole("option")).map((option) =>
      option.querySelector("[data-host-color]")?.getAttribute("data-host-color"),
    )
    expect(hues).toEqual([undefined, "grey", "teal"])
    await user.click(screen.getByRole("option", { name: "box" }))
    expect(
      screen
        .getByRole("combobox", { name: "Host" })
        .querySelector("[data-host-color]")
        ?.getAttribute("data-host-color"),
    ).toBe("teal")
  })

  it("opens on the host the header picker names", async () => {
    hostRows = [LOCAL, BOX]
    useSettingsStore.setState({ hostFilter: "box" })
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    const iframe = await screen.findByTitle("Default dashboard")
    expect(
      new URL(iframe.getAttribute("src") ?? "", "http://localhost").searchParams.get("host"),
    ).toBe("box")
  })

  it("shows no host select with local alone", async () => {
    hostRows = [LOCAL]
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    await screen.findByTitle("Default dashboard")
    expect(screen.queryByRole("combobox", { name: "Host" })).toBeNull()
  })

  it("lists a host it cannot reach as a disabled option with its reason, and opens on All over such a pick", async () => {
    const user = userEvent.setup()
    hostRows = [LOCAL, { ...BOX, status: "down", endpoint: null, error: "ssh closed" }]
    useSettingsStore.setState({ hostFilter: "box" })
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    const iframe = await screen.findByTitle("Default dashboard")
    expect(
      new URL(iframe.getAttribute("src") ?? "", "http://localhost").searchParams.get("host"),
    ).toBe("all")

    await user.click(screen.getByRole("combobox", { name: "Host" }))
    const option = await screen.findByRole("option", { name: /^box/ })
    expect(option.textContent).toBe("box (not reachable: ssh closed)")
    expect(option.getAttribute("aria-disabled")).toBe("true")
  })

  it("keeps a chosen host when it drops and returns, says why, and neither switches the iframe nor reloads it", async () => {
    const user = userEvent.setup()
    hostRows = [LOCAL, BOX]
    const { queryClient } = renderScreen(<DashboardsPage />, { route: "/dashboards" })
    await screen.findByTitle("Default dashboard")
    await user.click(screen.getByRole("combobox", { name: "Host" }))
    await user.click(await screen.findByRole("option", { name: "box" }))
    const iframe = await screen.findByTitle("Default dashboard")
    const src = iframe.getAttribute("src")
    const post = vi.spyOn((iframe as HTMLIFrameElement).contentWindow as Window, "postMessage")
    const listed = requests.filter((one) => one.path === "/v1/dashboards").length

    act(() =>
      queryClient.setQueryData(qk.hosts.list(), [
        LOCAL,
        { ...BOX, status: "down", endpoint: null, error: "ssh closed" },
      ]),
    )
    expect(await screen.findByText("Host box is not reachable: ssh closed")).toBeTruthy()
    expect(screen.getByTitle("Default dashboard")).toBe(iframe)
    expect(iframe.getAttribute("src")).toBe(src)

    act(() =>
      queryClient.setQueryData(qk.hosts.list(), [
        LOCAL,
        { ...BOX, endpoint: "http://box.test:9000" },
      ]),
    )
    await vi.waitFor(() => expect(screen.queryByText(/is not reachable/)).toBeNull())
    expect(screen.getByTitle("Default dashboard")).toBe(iframe)
    expect(iframe.getAttribute("src")).toBe(src)
    expect(post).not.toHaveBeenCalled()
    expect(requests.filter((one) => one.path === "/v1/dashboards")).toHaveLength(listed)
  })

  it("keeps the header's host when it drops and returns before a pick here, and says why", async () => {
    hostRows = [LOCAL, BOX]
    useSettingsStore.setState({ hostFilter: "box" })
    const { queryClient } = renderScreen(<DashboardsPage />, { route: "/dashboards" })
    const iframe = await screen.findByTitle("Default dashboard")
    const src = iframe.getAttribute("src")
    expect(src).toContain("host=box")

    act(() =>
      queryClient.setQueryData(qk.hosts.list(), [
        LOCAL,
        { ...BOX, status: "down", endpoint: null, error: "ssh closed" },
      ]),
    )
    expect(await screen.findByText("Host box is not reachable: ssh closed")).toBeTruthy()
    expect(screen.getByTitle("Default dashboard")).toBe(iframe)
    expect(iframe.getAttribute("src")).toBe(src)

    act(() => queryClient.setQueryData(qk.hosts.list(), [LOCAL, BOX]))
    await vi.waitFor(() => expect(screen.queryByText(/is not reachable/)).toBeNull())
    expect(screen.getByTitle("Default dashboard")).toBe(iframe)
    expect(iframe.getAttribute("src")).toBe(src)
  })

  it("follows the header pick only until a host is picked on the screen", async () => {
    const user = userEvent.setup()
    hostRows = [LOCAL, BOX]
    const hostOf = (iframe: HTMLElement) =>
      new URL(iframe.getAttribute("src") ?? "", "http://localhost").searchParams.get("host")
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    expect(hostOf(await screen.findByTitle("Default dashboard"))).toBe("all")
    act(() => useSettingsStore.setState({ hostFilter: "box" }))
    await vi.waitFor(() => expect(hostOf(screen.getByTitle("Default dashboard"))).toBe("box"))

    await user.click(screen.getByRole("combobox", { name: "Host" }))
    await user.click(await screen.findByRole("option", { name: "local" }))
    expect(hostOf(await screen.findByTitle("Default dashboard"))).toBe("local")
    act(() => useSettingsStore.setState({ hostFilter: "" }))
    expect(hostOf(screen.getByTitle("Default dashboard"))).toBe("local")
  })

  it("sends no stale host when the saved pick is no longer listed", async () => {
    hostRows = [LOCAL, BOX]
    useSettingsStore.setState({ hostFilter: "gone" })
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    const iframe = await screen.findByTitle("Default dashboard")
    expect(
      new URL(iframe.getAttribute("src") ?? "", "http://localhost").searchParams.get("host"),
    ).toBe("all")
  })

  it("disables Rename and Delete on the built-in dashboard, and enables them on one of your own", async () => {
    const user = userEvent.setup()
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    await screen.findByTitle("Default dashboard")

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
    renderScreen(<DashboardsPage />, { route: "/dashboards" })
    await screen.findByTitle("Default dashboard")

    await user.click(screen.getByRole("button", { name: "New dashboard" }))
    const createDialog = screen.getByRole("dialog", { name: "New dashboard" })
    await user.type(within(createDialog).getByLabelText("Name"), "on-call")
    await user.click(within(createDialog).getByRole("button", { name: "Create dashboard" }))

    expect(await screen.findByTitle("on-call dashboard")).toBeDefined()
  })

  it("renames a dashboard and opens the picker on the new name", async () => {
    const user = userEvent.setup()
    renderScreen(<DashboardsPage />, { route: "/dashboards?dashboard=custom" })
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
    renderScreen(<DashboardsPage />, { route: "/dashboards?dashboard=custom" })
    await screen.findByTitle("custom dashboard")

    await user.click(screen.getByRole("button", { name: "Delete" }))
    const confirm = await screen.findByRole("dialog", { name: "Delete custom?" })
    await user.click(within(confirm).getByRole("button", { name: "Delete" }))

    expect(await screen.findByTitle("Default dashboard")).toBeDefined()
    expect(requests).toContainEqual({ method: "DELETE", path: "/v1/dashboards/custom" })
  })
})
