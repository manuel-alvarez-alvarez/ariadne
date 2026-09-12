// @vitest-environment jsdom

import { fireEvent, screen, waitFor } from "@testing-library/react"
import { beforeEach, expect, it, vi } from "vitest"

import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { FakeWebSocket, stubWebSocket } from "@/test/web-socket"

import { SessionPanel } from "./session-panel"

const SESSION_ID = "01JSESS0000000000000000001"
const SESSION = aSession({ id: SESSION_ID })
const GOAL = aGoal({ id: SESSION.goal_id })
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
