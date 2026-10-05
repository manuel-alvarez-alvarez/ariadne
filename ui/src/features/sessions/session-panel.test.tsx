// @vitest-environment jsdom

import { fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { FakeWebSocket, stubWebSocket } from "@/test/web-socket"

import { SessionPanel } from "./session-panel"

const SESSION_ID = "01JSESS0000000000000000001"
const SESSION = aSession({ id: SESSION_ID })
const GOAL = aGoal({ id: SESSION.goal_id ?? "" })
const TASK = aTask({ id: SESSION.task_id ?? "01JTASK0000000000000000001", goal_id: GOAL.id })

beforeEach(() => {
  stubWebSocket()
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body = url.pathname.startsWith("/v1/sessions")
      ? SESSION
      : url.pathname.startsWith("/v1/goals")
        ? GOAL
        : TASK
    return Promise.resolve(jsonResponse(body))
  })
})

it("closes the panel when focused Escape reaches its console", async () => {
  const onClose = vi.fn()
  renderScreen(<SessionPanel sessionId={SESSION_ID} onClose={onClose} />)

  await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1))
  const textarea = screen.getByLabelText("Console terminal").querySelector("textarea")
  if (!textarea) throw new Error("the terminal has no textarea to type into")

  fireEvent.keyDown(textarea, { key: "Escape", code: "Escape" })

  await waitFor(() => expect(onClose).toHaveBeenCalledOnce())
})

/**
 * The shared header's own rows, top to bottom: the title (truncating rather
 * than wrapping, with the full text as its `title` attribute), then the meta
 * row of status, id and the stamps — the standalone session panel opens on no
 * breadcrumb, since nothing is open behind it.
 */
it("opens on the shared header: a truncating title, then status, id and stamps", async () => {
  renderScreen(<SessionPanel sessionId={SESSION_ID} onClose={() => {}} />)

  const title = await screen.findByRole("heading", { name: "Author session" })
  expect(title.className).toContain("truncate")
  expect(title.getAttribute("title")).toBe("Author session")

  const header = title.parentElement?.parentElement
  if (!header) throw new Error("no header around the title")
  const [titleRow, meta] = [...header.children]
  expect(titleRow instanceof HTMLElement && titleRow.contains(title)).toBe(true)
  if (!(meta instanceof HTMLElement)) throw new Error("no meta row")
  // Status first, then the id, then the stamps.
  const text = meta.textContent ?? ""
  const statusAt = text.indexOf("Running")
  const idAt = text.indexOf(SESSION_ID)
  const startedAt = text.indexOf("started")
  expect(statusAt).toBeGreaterThanOrEqual(0)
  expect(statusAt).toBeLessThan(idAt)
  expect(idAt).toBeLessThan(startedAt)
})

it("keeps the console inside the remaining pane height", async () => {
  renderScreen(<SessionPanel sessionId={SESSION_ID} onClose={() => {}} />)
  const console = await screen.findByLabelText("Console terminal")
  const body = console.closest('[data-slot="pane-body"]')
  expect(body?.className).toContain("min-h-0")
  expect(body?.className).toContain("flex-1")
  expect(body?.className).toContain("overflow-y-auto")
  const bodyContent = body?.firstElementChild
  expect(bodyContent?.className).toContain("h-full")
  expect(bodyContent?.className).toContain("flex")

  const view = bodyContent?.firstElementChild
  expect(view?.className).toContain("h-full")
  expect(view?.className).toContain("min-h-0")
  expect(view?.className).toContain("flex")

  const tabs = view?.querySelector('[data-slot="tabs"]')
  expect(tabs?.className).toContain("min-h-0")
  expect(tabs?.className).toContain("flex")

  const content = console.closest('[data-slot="tabs-content"]')
  expect(content?.className).toContain("min-h-0")
  expect(content?.className).toContain("flex-col")
  const terminalBox = console.parentElement?.parentElement?.parentElement
  expect(terminalBox?.className).toContain("min-h-0")
  expect(terminalBox?.className).toContain("flex")
  expect(terminalBox?.className).not.toContain("min-h-[24rem]")
})

it("keeps modal Escape separate from the pane close", async () => {
  const onClose = vi.fn()
  renderScreen(<SessionPanel sessionId={SESSION_ID} onClose={onClose} />)
  const user = userEvent.setup()
  await user.click(await screen.findByRole("button", { name: "Expand the console" }))
  const dialog = await screen.findByRole("dialog")
  await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2))
  const textarea = dialog.querySelector("textarea")
  if (!textarea) throw new Error("no console keyboard")
  fireEvent.keyDown(textarea, { key: "Escape", code: "Escape" })
  expect(screen.getByRole("dialog")).toBe(dialog)
  expect(onClose).not.toHaveBeenCalled()
  fireEvent.keyDown(
    screen.getByRole("button", { name: "Collapse the console back into the panel" }),
    {
      key: "Escape",
      code: "Escape",
    },
  )
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  expect(onClose).not.toHaveBeenCalled()
  // The pane's own name is the session's heading, rendered by the shared
  // header now rather than left sr-only behind it.
  expect(screen.getByRole("region", { name: "Author session" })).toBeDefined()
})
