// @vitest-environment jsdom

import { fireEvent, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"
import { AgentsPage } from "@/features/agents/agents-page"
import { AttentionPopover } from "@/features/goals/attention-strip"
import { IssuesPage } from "@/features/issues/issues-page"
import { SessionsPage } from "@/features/sessions/sessions-page"
import { useSettingsStore } from "@/stores/settings"
import { anAgentConfig, aSession, aSessionPage } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"

beforeEach(() => useSettingsStore.setState({ hostFilter: "" }))
afterEach(() => forgetHosts())

it("keeps session rows and skeletons while another host loads", async () => {
  stubHosts({ local: { "/v1/sessions": aSessionPage([aSession()]) } })
  renderScreen(<SessionsPage />)
  await screen.findByRole("row", { name: "Loading title" })
  expect(
    screen.getByRole("row", { name: "Loading title" }).querySelector('[data-slot="skeleton"]'),
  ).toBeTruthy()
  expect(screen.queryByText(/of .* sessions/)).toBeNull()
  expect(screen.queryByText("No sessions found.")).toBeNull()
  expect(document.querySelectorAll('tbody [data-slot="skeleton"]').length).toBeGreaterThan(1)
})

it("keeps skeleton tabs for hosts that have not answered", async () => {
  stubHosts({ local: { "/v1/agents": [anAgentConfig()], "/v1/models": [], "/v1/acp-agents": [] } })
  renderScreen(<AgentsPage />)
  await screen.findByRole("tab", { name: /claude/ })
  expect(screen.getByLabelText("Loading agents on box")).toBeTruthy()
})

it("keeps attention unresolved after one empty host answer", async () => {
  const seen = stubHosts({
    local: {
      "/v1/goals": [],
      "/v1/tasks": [],
      "/v1/sessions": aSessionPage([]),
      "/v1/attention": { items: [], complete: true },
    },
  })
  renderScreen(<AttentionPopover />)
  await waitFor(() =>
    expect(
      seen.some(
        (request) => request.url.pathname === "/v1/attention" && request.origin.includes("box"),
      ),
    ).toBe(true),
  )
  expect(screen.getByRole("button", { name: "Needs attention, loading" })).toBeTruthy()
})

it("keeps issues unresolved until every host lists its repositories", async () => {
  const seen = stubHosts({ local: { "/v1/repositories": [] } })
  renderScreen(<IssuesPage />)
  await waitFor(() =>
    expect(
      seen.some(
        (request) => request.url.pathname === "/v1/repositories" && request.origin.includes("box"),
      ),
    ).toBe(true),
  )
  expect(document.querySelector('[data-slot="skeleton"]')).toBeTruthy()
  expect(screen.queryByText(/No issues/)).toBeNull()
})

it("hides the disabled model count until the catalog answers", async () => {
  stubHosts({ local: { "/v1/agents": [anAgentConfig()], "/v1/acp-agents": [] } })
  renderScreen(<AgentsPage />)
  await screen.findByRole("tab", { name: /claude/ })
  fireEvent.mouseEnter(screen.getByRole("button", { name: "Refresh" }))
  expect(await screen.findByLabelText("Loading model count")).toBeTruthy()
  expect(screen.queryByText(/0 models turned off/)).toBeNull()
})
