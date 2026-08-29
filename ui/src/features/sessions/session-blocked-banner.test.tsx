// @vitest-environment jsdom

/**
 * The line that says what a blocked agent is waiting for, and the button that
 * answers the common case.
 *
 * What is worth pinning down is that it appears for exactly the two reasons a
 * *pane* is waiting on — a question asked in a thread is answered somewhere
 * else entirely — that Approve sends the keystroke and not something a person
 * has to type, and that a pane that is gone offers no button to type into it.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { aSession } from "@/test/fixtures"
import { daemonFetch, renderScreen } from "@/test/harness"

import { SessionBlockedBanner } from "./session-blocked-banner"

const SESSION = aSession({
  id: "01JSESS0000000000000000001",
  status: "running",
  attention_reason: "waiting_permission",
})

it("says what a permission prompt is waiting for, and offers to answer it", async () => {
  daemonFetch.mockResolvedValue(new Response(null, { status: 204 }))
  renderScreen(<SessionBlockedBanner session={SESSION} />)

  expect(screen.getByText("Blocked on a permission prompt")).not.toBeNull()

  await userEvent.click(screen.getByRole("button", { name: "Approve" }))

  await waitFor(() => expect(daemonFetch).toHaveBeenCalledTimes(1))
  const request = daemonFetch.mock.calls[0]?.[0] as Request
  expect(request.url).toContain(`/v1/sessions/${SESSION.id}/input`)
  // The keystroke a person would have typed, exactly: `y`, then Return.
  expect(JSON.parse(await request.text()).data).toBe("y\r")
})

// The pane is waiting on an answer only the user can write; there is no one
// key that is it.
it("asks for an answer without offering one when the agent asked a question", () => {
  renderScreen(<SessionBlockedBanner session={{ ...SESSION, attention_reason: "waiting_input" }} />)

  expect(screen.getByText("The agent asked a question")).not.toBeNull()
  expect(screen.queryByRole("button", { name: "Approve" })).toBeNull()
})

// The reason outlives the pane: a session that died while blocked still
// carries it, and there is nothing left to type into.
it("says the pane is gone rather than offering to type into it", () => {
  renderScreen(<SessionBlockedBanner session={{ ...SESSION, status: "failed" }} />)

  expect(screen.getByText(/resume the session first/)).not.toBeNull()
  expect(screen.queryByRole("button", { name: "Approve" })).toBeNull()
})

// Everything else on the attention list is answered somewhere other than the
// pane — a thread, a retry — so the pane says nothing about it.
it("stays out of the way of every other reason", () => {
  for (const reason of ["waiting_user", "agent_error", "disconnected", "stalled", null] as const) {
    renderScreen(<SessionBlockedBanner session={{ ...SESSION, attention_reason: reason }} />)
    expect(screen.queryByRole("alert")).toBeNull()
  }
})
