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

it("gives the console view the remaining pane height", async () => {
  renderScreen(<SessionPanel sessionId={SESSION_ID} onClose={() => {}} />)
  const console = await screen.findByLabelText("Console terminal")
  const body = console.closest('[data-slot="pane-body"]')
  expect(body?.className).toContain("min-h-0")
  expect(body?.className).toContain("flex-1")
  expect(body?.className).toContain("overflow-y-auto")
  expect(body?.firstElementChild?.className).toContain("h-full")
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
  expect(screen.getByRole("region", { name: /^Session / })).toBeDefined()
})
