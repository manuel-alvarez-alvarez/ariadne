// @vitest-environment jsdom

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import type { ModelDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { aModel, anEffort, aSession } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"

import { SessionActions } from "./session-actions"

const SESSION = aSession({ id: "01JSESS000000000000000OLD" })
const SUCCESSOR = aSession({
  id: "01JSESS000000000000000NEW",
  model: "codex-acp:gpt-5.3-codex",
  effort: "high",
})
const MODELS: ModelDto[] = [
  aModel({
    id: SUCCESSOR.model,
    agent_id: "codex-acp",
    efforts: [anEffort({ id: "high", default: true })],
  }),
]

const bodies: unknown[] = []

function stubDaemon(reply: () => Response) {
  bodies.length = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const { pathname } = new URL(request.url)
    if (pathname === `/v1/sessions/${SESSION.id}/switch`) {
      bodies.push(await request.json())
      return reply()
    }
    if (pathname === "/v1/models") return jsonResponse(MODELS)
    return jsonResponse([])
  })
}

async function switchToSuccessor(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Switch" }))
  await user.click(await screen.findByRole("button", { name: "Runs on" }))
  const options = await screen.findByRole("listbox", { name: "Models" })
  await user.click(within(options).getByText(SUCCESSOR.model))
  await user.click(screen.getByRole("radio", { name: "high" }))
  await user.keyboard("{Escape}")
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Switch" }))
}

it("opens the switch dialog, posts its pin, and follows a successor", async () => {
  stubDaemon(() => jsonResponse(SUCCESSOR))
  const onSwitched = vi.fn()
  const user = userEvent.setup()
  renderScreen(<SessionActions session={SESSION} onSwitched={onSwitched} />)

  await switchToSuccessor(user)

  await waitFor(() => expect(onSwitched).toHaveBeenCalledWith(SUCCESSOR))
  expect(bodies).toEqual([{ model: SUCCESSOR.model, effort: "high" }])
})

it("shows the daemon refusal in the switch dialog", async () => {
  stubDaemon(() =>
    errorResponse(409, "illegal_transition", "cannot switch a session of a cancelled goal"),
  )
  const user = userEvent.setup()
  renderScreen(
    <>
      <SessionActions session={SESSION} />
      <Toaster />
    </>,
  )

  await switchToSuccessor(user)

  await waitFor(() =>
    expect(screen.getByLabelText(/Notifications/).textContent).toContain(
      "cannot switch a session of a cancelled goal",
    ),
  )
})

it("keeps the selected session when the daemon returns its id", async () => {
  stubDaemon(() => jsonResponse({ ...SESSION, model: SUCCESSOR.model, effort: "high" }))
  const onSwitched = vi.fn()
  const user = userEvent.setup()
  renderScreen(<SessionActions session={SESSION} onSwitched={onSwitched} />)

  await switchToSuccessor(user)

  await waitFor(() => expect(bodies).toHaveLength(1))
  expect(onSwitched).not.toHaveBeenCalled()
})
