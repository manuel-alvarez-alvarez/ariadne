// @vitest-environment jsdom

/**
 * The drawer's contract with the shell: the footer's status button opens it,
 * the stream lives exactly as long as it is open, and closing gives focus back
 * to the button that opened it (the dialog primitive's side of the bargain,
 * pinned here because the whole feature leans on it).
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { beforeEach, expect, it, vi } from "vitest"

import { AppShell } from "@/components/app-shell"
import { TooltipProvider } from "@/components/ui/tooltip"
import type { Connection } from "@/hooks/use-connection"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"

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
})

function mountShell() {
  const router = createMemoryRouter([
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
      },
    ],
  })
  source.emit("delta", {
    ts: "2026-08-18T12:00:01.000000Z",
    level: "WARN",
    target: "ariadne_daemon::http",
    message: "slow request",
  })
  expect(await screen.findByText("tick complete tasks=3")).toBeTruthy()
  expect(screen.getByText("slow request")).toBeTruthy()

  await user.keyboard("{Escape}")
  expect(screen.queryByRole("heading", { name: "Daemon logs" })).toBeNull()
  expect(source.closed).toBe(true)
  // The dialog primitive returns focus to its opener: the footer button.
  expect(document.activeElement).toBe(footerButton)
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
