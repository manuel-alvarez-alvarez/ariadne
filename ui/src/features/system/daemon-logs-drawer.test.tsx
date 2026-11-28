// @vitest-environment jsdom

/**
 * The drawer's contract with the shell: the footer's status button opens it,
 * the stream lives exactly as long as it is open, and closing gives focus back
 * to the button that opened it (the dialog primitive's side of the bargain,
 * pinned here because the whole feature leans on it).
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createBrowserRouter, RouterProvider } from "react-router-dom"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { AppShell } from "@/components/app-shell"
import { TooltipProvider } from "@/components/ui/tooltip"
import { dispatchDomainEvent } from "@/events/dispatch"
import type { useConnection } from "@/hooks/use-connection"
import { byContent } from "@/test/text-content"

type Connection = ReturnType<typeof useConnection>

import { useSettingsStore } from "@/stores/settings"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { startAt } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"

vi.mock("@/hooks/use-connection", () => ({
  useConnection: (): Connection => ({
    status: "connected",
    baseUrl: "http://127.0.0.1:7676",
    version: "0.3.1",
    uptimeSecs: 60,
    error: null,
    retry: () => {},
  }),
}))

beforeEach(() => {
  stubEventSource()
  stubHosts({}, [LOCAL])
})

afterEach(() => {
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

let lastClient: QueryClient

function mountShell() {
  startAt("/")
  const router = createBrowserRouter([
    { path: "/", element: <AppShell />, children: [{ index: true, element: <div /> }] },
  ])
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <TooltipProvider delay={0}>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  )
  lastClient = client
  return screen.getByRole("button", { name: "Daemon status — open logs" })
}

it("opens from the footer, tails the stream, and lets go of it on close", async () => {
  const user = userEvent.setup()
  const footerButton = mountShell()

  // Closed drawer, no connection: the stream must not outlive the view.
  expect(FakeEventSource.instances).toHaveLength(0)

  await user.click(footerButton)
  expect(await screen.findByRole("heading", { name: "Daemon logs" })).toBeTruthy()
  expect(FakeEventSource.instances).toHaveLength(1)
  const source = latestSource()
  expect(source.url).toBe("http://127.0.0.1:7676/v1/logs/stream")

  source.emit("snapshot", {
    lines: [
      {
        ts: "2026-08-18T12:00:00.000000Z",
        level: "INFO",
        target: "ariadne_daemon::scheduler",
        message: "tick complete tasks=3",
        service: "ariadned",
      },
    ],
  })
  source.emit("delta", {
    ts: "2026-08-18T12:00:01.000000Z",
    level: "WARN",
    target: "ariadne_daemon::http",
    message: "slow request",
    service: "ariadned",
  })
  expect(await screen.findByText("tick complete tasks=3")).toBeTruthy()
  expect(screen.getByText("slow request")).toBeTruthy()

  await user.keyboard("{Escape}")
  expect(screen.queryByRole("heading", { name: "Daemon logs" })).toBeNull()
  expect(source.closed).toBe(true)
  // The dialog primitive returns focus to its opener: the footer button.
  expect(document.activeElement).toBe(footerButton)
})

it("reconnects with a service and level filter when one is picked", async () => {
  const user = userEvent.setup()
  const footerButton = mountShell()

  await user.click(footerButton)
  expect(FakeEventSource.instances).toHaveLength(1)
  expect(latestSource().url).toBe("http://127.0.0.1:7676/v1/logs/stream")

  await user.click(screen.getByRole("combobox", { name: "Filter by service" }))
  await user.click(await screen.findByRole("option", { name: "ariadne-cli" }))
  expect(FakeEventSource.instances).toHaveLength(2)
  expect(latestSource().url).toBe("http://127.0.0.1:7676/v1/logs/stream?service=ariadne-cli")

  await user.click(screen.getByRole("combobox", { name: "Filter by level" }))
  await user.click(await screen.findByRole("option", { name: "Warn" }))
  expect(FakeEventSource.instances).toHaveLength(3)
  expect(latestSource().url).toBe(
    "http://127.0.0.1:7676/v1/logs/stream?service=ariadne-cli&level=warn",
  )
})

it("says when it has nothing to show yet", async () => {
  const user = userEvent.setup()
  const footerButton = mountShell()

  await user.click(footerButton)
  const source = latestSource()
  source.onopen?.()
  source.emit("snapshot", { lines: [] })

  expect(await screen.findByText("Nothing logged yet.")).toBeTruthy()
})

it("says out loud when the stream drops", async () => {
  const user = userEvent.setup()
  const footerButton = mountShell()

  await user.click(footerButton)
  const source = latestSource()
  source.onopen?.()
  source.onerror?.()

  expect(await screen.findByText("Log stream lost — reconnecting")).toBeTruthy()
})

it("opens the log of the picked host, and of local under All, and follows the drawer's own host select", async () => {
  const user = userEvent.setup()
  stubHosts({})
  useSettingsStore.setState({ hostFilter: "box" })
  const footerButton = mountShell()

  await user.click(footerButton)
  // The picker's pick waits for the hosts list, then opens that host's stream.
  await vi.waitFor(() => expect(latestSource().url).toBe("http://box.test:7676/v1/logs/stream"))

  await user.click(screen.getByRole("combobox", { name: "Host" }))
  await user.click(await screen.findByRole("option", { name: "local" }))
  expect(latestSource().url).toBe("http://127.0.0.1:7676/v1/logs/stream")
})

it("draws each host of the drawer's host select with its colour dot", async () => {
  const user = userEvent.setup()
  stubHosts({}, [LOCAL, { ...BOX, color: "violet" }])
  useSettingsStore.setState({ hostFilter: "box" })
  const footerButton = mountShell()

  await user.click(footerButton)
  const select = await screen.findByRole("combobox", { name: "Host" })
  await vi.waitFor(() =>
    expect(select.querySelector("[data-host-color]")?.getAttribute("data-host-color")).toBe(
      "violet",
    ),
  )
  await user.click(select)
  const hues = (await screen.findAllByRole("option")).map((option) =>
    option.querySelector("[data-host-color]")?.getAttribute("data-host-color"),
  )
  expect(hues).toEqual(["grey", "violet"])
})

it("defaults to local under All hosts", async () => {
  const user = userEvent.setup()
  stubHosts({})
  const footerButton = mountShell()

  await user.click(footerButton)
  expect(latestSource().url).toBe("http://127.0.0.1:7676/v1/logs/stream")
})

const BOX_DOWN = { ...BOX, status: "down" as const, endpoint: null, error: "ssh closed" }

it("shows no host select while the daemon lists local alone", async () => {
  const user = userEvent.setup()
  stubHosts({}, [LOCAL])
  const footerButton = mountShell()

  await user.click(footerButton)
  expect(latestSource().url).toBe("http://127.0.0.1:7676/v1/logs/stream")
  expect(screen.queryByRole("combobox", { name: "Host" })).toBeNull()
})

it("lists a host it cannot reach as a disabled option with its reason, and defaults to local over such a pick", async () => {
  const user = userEvent.setup()
  stubHosts({}, [LOCAL, BOX_DOWN])
  useSettingsStore.setState({ hostFilter: "box" })
  const footerButton = mountShell()

  await user.click(footerButton)
  await user.click(await screen.findByRole("combobox", { name: "Host" }))
  const option = await screen.findByRole("option", { name: /^box/ })
  expect(option.textContent).toBe("box (not reachable: ssh closed)")
  expect(option.getAttribute("aria-disabled")).toBe("true")
  expect(latestSource().url).toBe("http://127.0.0.1:7676/v1/logs/stream")
})

it("keeps the host it opened on when that host goes down, and says why", async () => {
  const user = userEvent.setup()
  // The daemon's own row moves first, as in production: a list read that lands
  // after the event answers the same row, so the event cannot be overwritten.
  const hosts = [LOCAL, BOX]
  stubHosts({}, hosts)
  useSettingsStore.setState({ hostFilter: "box" })
  const footerButton = mountShell()

  await user.click(footerButton)
  await vi.waitFor(() => expect(latestSource().url).toBe("http://box.test:7676/v1/logs/stream"))

  const hueOfName = () =>
    within(screen.getByText(byContent("Host box is not reachable: ssh closed")))
      .getByText("box")
      .getAttribute("data-host-color")
  hosts[1] = BOX_DOWN
  act(() => {
    dispatchDomainEvent(lastClient, "local", { event: "host_updated", data: BOX_DOWN })
  })
  expect(await screen.findByText(byContent("Host box is not reachable: ssh closed"))).toBeTruthy()
  expect(hueOfName()).not.toBeNull()
  const pink = { ...BOX_DOWN, color: "pink" as const }
  hosts[1] = pink
  act(() => {
    dispatchDomainEvent(lastClient, "local", { event: "host_updated", data: pink })
  })
  await vi.waitFor(() => expect(hueOfName()).toBe("pink"))
})
