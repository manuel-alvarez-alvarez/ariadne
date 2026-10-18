// @vitest-environment jsdom

/**
 * The sidebar foot's daemon-status button, in every state the connection can
 * be in, and its rail variant — the dot alone.
 *
 * `useConnection` is mocked because the states under test are exactly its
 * outputs, and driving a real event stream through them would test the mock
 * traffic rather than the readout — how the stream produces those states is
 * `events/provider.test.tsx`. The dot's tone-and-pulse rules are the semantics
 * the old sidebar badge had, so they are asserted literally; where the
 * indicator lives now — the sidebar's own foot, and no footer of the shell's
 * own — is `app-shell.test.tsx`'s to pin down.
 */

import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import type { useConnection } from "@/hooks/use-connection"

import { ConnectionStatus } from "./connection-status"

type Connection = ReturnType<typeof useConnection>

/** What the mocked `useConnection` answers; each test writes its own. */
const state = vi.hoisted(() => ({ current: undefined as unknown }))

vi.mock("@/hooks/use-connection", () => ({
  useConnection: () => state.current as Connection,
}))

// `globals` is off, so nothing unmounts a screen between tests but this.

function conn(over: Partial<Connection> = {}): Connection {
  return {
    status: "connected",
    baseUrl: "http://127.0.0.1:7676",
    version: "0.3.1",
    uptimeSecs: 4200,
    error: null,
    retry: () => {},
    ...over,
  }
}

function mountStatus(over: Partial<Connection> = {}, { collapsed = false } = {}) {
  state.current = conn(over)
  render(
    <TooltipProvider delay={0}>
      <ConnectionStatus collapsed={collapsed} />
    </TooltipProvider>,
  )
  return screen.getByRole("button", { name: /^Daemon status/ })
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

it("shows warn while the first connection is still being made", () => {
  const button = mountStatus({ status: "connecting", version: null, uptimeSecs: null })
  expect(button.textContent).toContain("connecting…")
  expect(dot(button).contains("bg-status-warn")).toBe(true)
})

it("shows danger once the daemon is unreachable, and stops pulsing", () => {
  const button = mountStatus({ status: "disconnected", uptimeSecs: null })
  expect(button.textContent).toContain("disconnected")
  expect(dot(button).contains("bg-status-danger")).toBe(true)
  expect(dot(button).contains("animate-pulse")).toBe(false)
})

it("details the URL, the uptime and the error in its tooltip", async () => {
  mountStatus()
  const user = userEvent.setup()
  await user.tab()

  expect(await screen.findByText("http://127.0.0.1:7676")).toBeTruthy()
  expect(screen.getByText("Daemon: connected · up 1h")).toBeTruthy()
})

it("says why the connection went, once it is gone", async () => {
  mountStatus({ status: "disconnected", uptimeSecs: null, error: "no heartbeat from the daemon" })
  const user = userEvent.setup()
  await user.tab()

  expect(await screen.findByText("Daemon: disconnected")).toBeTruthy()
  expect(screen.getByText("no heartbeat from the daemon")).toBeTruthy()
})

it("shows the dot alone in the rail, named by a tooltip instead of a label", async () => {
  const button = mountStatus({}, { collapsed: true })

  expect(button.textContent).toBe("")
  expect(dot(button).contains("bg-status-done")).toBe(true)
  expect(button.getAttribute("aria-label")).toBe("Daemon status: ariadned 0.3.1")

  const user = userEvent.setup()
  await user.hover(button)
  expect(await screen.findByText("ariadned 0.3.1")).toBeTruthy()
})
