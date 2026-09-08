// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import { anAgentEvent } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SessionActivity } from "./session-activity"

const EVENT = anAgentEvent({
  summary: "Read the repository guidance.",
  payload: { cwd: "/Users/me/dev/ariadne", tool_name: "Read" },
})

beforeEach(() => {
  daemonFetch.mockImplementation(async () => jsonResponse([EVENT]))
})

function renderActivity() {
  renderScreen(<SessionActivity sessionId={EVENT.session_id ?? ""} />)
}

function row() {
  const button = screen.getByText(EVENT.summary).closest("button")
  if (!button) throw new Error("event summary has no row")
  return button
}

it("shows the summary the daemon sent", async () => {
  renderActivity()

  expect(await screen.findByText(EVENT.summary)).toBeDefined()
  expect(screen.queryByText(/cwd/)).toBeNull()
})

it("opens and closes an event payload under its row", async () => {
  const user = userEvent.setup()
  renderActivity()
  await screen.findByText(EVENT.summary)

  await user.click(row())
  expect(screen.getByRole("region", { name: "post_tool_use payload" }).textContent).toContain(
    '"cwd": "/Users/me/dev/ariadne"',
  )

  await user.click(row())
  expect(screen.queryByRole("region", { name: "post_tool_use payload" })).toBeNull()
})
