// @vitest-environment jsdom

/**
 * The footer's daemon-status button, in every state the connection can be in.
 *
 * `useConnection` is mocked because the states under test are exactly its
 * outputs — the two links folded together — and driving a real health probe
 * and event stream into "REST up, stream down" would test the mock traffic,
 * not the readout. The dot's tone-and-pulse rules are the semantics the old
 * sidebar badge had, so they are asserted literally; the shell case pins down
 * where the indicator now lives — the footer, and no longer the sidebar.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { afterEach, expect, it, vi } from "vitest"

import type { ApiError } from "@/api"
import { AppShell } from "@/components/app-shell"
import { TooltipProvider } from "@/components/ui/tooltip"
import type { Connection } from "@/hooks/use-connection"

import { ConnectionStatus } from "./connection-status"

/**
 * Hoisted for the same reason `detail-panels.test.tsx` hoists its stub:
 * `openapi-fetch` takes its `fetch` when `@/api` is imported. The shell case
 * mounts the sidebar, whose attention badge does query — never settling keeps
 * it pending, which is all it needs to be.
 */
vi.hoisted(() => {
  globalThis.fetch = (() => new Promise(() => {})) as unknown as typeof fetch
})

/** What the mocked `useConnection` answers; each test writes its own. */
const state = vi.hoisted(() => ({ current: undefined as unknown }))

vi.mock("@/hooks/use-connection", () => ({
  useConnection: () => state.current as Connection,
}))

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(cleanup)

function conn(over: Partial<Connection> = {}): Connection {
  return {
    status: "connected",
    baseUrl: "http://127.0.0.1:7676",
    version: "0.3.1",
    uptimeSecs: 4200,
    error: null,
    streamStatus: "open",
    refetch: () => {},
    ...over,
  }
}

function mountStatus(over: Partial<Connection> = {}) {
  state.current = conn(over)
  render(
    <TooltipProvider delay={0}>
      <ConnectionStatus />
    </TooltipProvider>,
  )
  return screen.getByRole("button", { name: "Daemon status — open logs" })
}

/** The status dot — the button's one presentational span. */
function dot(button: HTMLElement): DOMTokenList {
  const el = button.querySelector("span[aria-hidden]")
  if (!el) throw new Error("no status dot in the button")
  return el.classList
}

it("shows a pulsing green dot and the daemon version when fully live", () => {
  const button = mountStatus()
  expect(button.textContent).toContain("ariadned 0.3.1")
  expect(dot(button).contains("bg-status-done")).toBe(true)
  expect(dot(button).contains("animate-pulse")).toBe(true)
})

it("steps down to warn, without the pulse, when only the stream is down", () => {
  const button = mountStatus({ streamStatus: "reconnecting" })
  expect(button.textContent).toContain("ariadned 0.3.1")
  expect(dot(button).contains("bg-status-warn")).toBe(true)
  expect(dot(button).contains("animate-pulse")).toBe(false)
})

it("shows warn while the first probe is still in flight", () => {
  const button = mountStatus({ status: "connecting", version: null, uptimeSecs: null })
  expect(button.textContent).toContain("connecting…")
  expect(dot(button).contains("bg-status-warn")).toBe(true)
})

it("shows danger once the daemon is unreachable", () => {
  const button = mountStatus({ status: "disconnected", uptimeSecs: null })
  expect(button.textContent).toContain("disconnected")
  expect(dot(button).contains("bg-status-danger")).toBe(true)
})

it("details the two links, uptime and the error in its tooltip", async () => {
  mountStatus({
    status: "disconnected",
    streamStatus: "reconnecting",
    error: { message: "connection refused" } as ApiError,
  })
  const user = userEvent.setup()
  await user.tab()

  expect(await screen.findByText("http://127.0.0.1:7676")).toBeTruthy()
  expect(screen.getByText("API: disconnected · up 1h")).toBeTruthy()
  expect(screen.getByText("Events: reconnecting")).toBeTruthy()
  expect(screen.getByText("connection refused")).toBeTruthy()
})

it("lives in the shell's footer, and no longer in the sidebar", () => {
  state.current = conn()
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

  const button = screen.getByRole("button", { name: "Daemon status — open logs" })
  expect(screen.getByRole("contentinfo").contains(button)).toBe(true)
  const sidebar = document.querySelector("aside")
  if (!sidebar) throw new Error("no sidebar in the shell")
  expect(sidebar.contains(button)).toBe(false)
})
