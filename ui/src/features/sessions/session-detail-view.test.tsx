// @vitest-environment jsdom

/** The fold of the session facts: in the panel heading's line, and remembered. */

import { cleanup, fireEvent, screen } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"
import { useSettingsStore } from "@/stores/settings"
import { stubEventSource } from "@/test/event-source"
import { aSession } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { SessionDetailView, SessionPanelHeader } from "./session-detail-view"

const session = aSession({ id: "01JSESS0000000000000000001", status: "idle" })

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ sessionFactsOpen: true })
  stubEventSource()
  daemonFetch.mockImplementation(async () => jsonResponse([]))
})

function renderPanel() {
  return renderScreen(
    <>
      <SessionPanelHeader host="local" session={session} />
      <SessionDetailView host="local" session={session} />
    </>,
  )
}

it("folds the session facts from a control in the line of the panel heading", () => {
  renderPanel()

  const toggle = screen.getByRole("button", { name: "Collapse session facts" })
  // The heading shares the line the control is in.
  expect(toggle.parentElement?.parentElement?.textContent).toContain("session")
  expect(screen.getByText("Agent session id")).toBeTruthy()

  fireEvent.click(toggle)

  expect(screen.queryByText("Agent session id")).toBeNull()
  expect(screen.getByRole("button", { name: "Expand session facts" })).toBeTruthy()
})

it("keeps the fold across a restart of the app", async () => {
  renderPanel()
  fireEvent.click(screen.getByRole("button", { name: "Collapse session facts" }))
  cleanup()

  // A restart: the store starts from its default and reads what was saved.
  const saved = localStorage.getItem("ariadne.settings") ?? ""
  useSettingsStore.setState({ sessionFactsOpen: true })
  localStorage.setItem("ariadne.settings", saved)
  await useSettingsStore.persist.rehydrate()

  renderPanel()
  expect(screen.getByRole("button", { name: "Expand session facts" })).toBeTruthy()
  expect(screen.queryByText("Agent session id")).toBeNull()
})

it("opens the facts when the user has not folded them", () => {
  renderPanel()
  expect(screen.getByRole("button", { name: "Collapse session facts" })).toBeTruthy()
})
