// @vitest-environment jsdom

/**
 * The line that says what a blocked agent is waiting for.
 *
 * What is worth pinning down is that it appears for exactly the two reasons
 * the console is waiting on — a question asked in a thread is answered
 * somewhere else entirely — and that an agent that is gone is not pointed at
 * a console it can no longer be answered in.
 */

import { screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { aSession } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"

import { SessionBlockedBanner } from "./session-blocked-banner"

const SESSION = aSession({
  id: "01JSESS0000000000000000001",
  status: "running",
  attention_reason: "waiting_permission",
})

it("says what a permission prompt is waiting for, and where to answer it", () => {
  renderScreen(<SessionBlockedBanner session={SESSION} />)

  expect(screen.getByText("Blocked on a permission prompt")).not.toBeNull()
  expect(screen.getByText(/options in the console below/)).not.toBeNull()
})

it("asks for an answer when the agent asked a question", () => {
  renderScreen(<SessionBlockedBanner session={{ ...SESSION, attention_reason: "waiting_input" }} />)

  expect(screen.getByText("The agent asked a question")).not.toBeNull()
  expect(screen.getByText(/into the console below/)).not.toBeNull()
})

// The reason outlives the agent: a session that died while blocked still
// carries it, and there is nothing left to answer.
it("says the agent is gone rather than pointing at the console", () => {
  renderScreen(<SessionBlockedBanner session={{ ...SESSION, status: "failed" }} />)

  expect(screen.getByText(/resume the session first/)).not.toBeNull()
  expect(screen.queryByText(/console below/)).toBeNull()
})

// Everything else on the attention list is answered somewhere other than the
// console — a thread, a retry — so the banner says nothing about it.
it("stays out of the way of every other reason", () => {
  for (const reason of ["waiting_user", "agent_error", "disconnected", "stalled", null] as const) {
    renderScreen(<SessionBlockedBanner session={{ ...SESSION, attention_reason: reason }} />)
    expect(screen.queryByRole("alert")).toBeNull()
  }
})
