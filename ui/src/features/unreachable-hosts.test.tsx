// @vitest-environment jsdom

/**
 * A host the app cannot reach, in the dialogs, cards and errors of the screens
 * that read hosts: it is named, with its reason, and never left loading.
 */

import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { ApiError, type HostDto } from "@/api"
import { DataTable } from "@/components/data-table"
import { useSettingsStore } from "@/stores/settings"
import { anAiPermissionsStatus, aSession, aSessionPage } from "@/test/fixtures"
import { errorResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { CreateGoalDialog } from "./goals/create-goal-dialog"
import { GoalsListPage } from "./goals/goals-list-page"
import { PermissionsPage } from "./permissions/permissions-page"
import { SessionsPage } from "./sessions/sessions-page"

/** `box`, down: the settings daemon lists it with its own reason. */
const DOWN: HostDto = { ...BOX, status: "down", error: "ssh: connection refused" }

const REASON = "ssh: connection refused"

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ hostFilter: "", goalStatusFilter: "" })
})
afterEach(() => {
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

const dialogAnswers = { "/v1/repositories": [], "/v1/models": [], "/v1/workflows": [] }

it("opens the goal dialog on local when the picked host cannot be reached", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  stubHosts({ local: dialogAnswers }, [LOCAL, DOWN])
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  const host = await screen.findByRole("combobox", { name: "Host" })
  await waitFor(() => expect(host.textContent).toContain("local"))
  expect(host.textContent).not.toContain("box")
})

it("lists an unreachable host in the goal dialog as a disabled option with its reason", async () => {
  const user = userEvent.setup()
  stubHosts({ local: dialogAnswers }, [LOCAL, DOWN])
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  await user.click(await screen.findByRole("combobox", { name: "Host" }))
  const option = await screen.findByRole("option", { name: new RegExp(`^box.*${REASON}`) })
  expect(option.getAttribute("aria-disabled")).toBe("true")
  expect(screen.getByRole("option", { name: "local" }).getAttribute("aria-disabled")).not.toBe(
    "true",
  )
})

it("shows the reason of an unreachable host on the AI tab, with no skeleton", async () => {
  stubHosts({ local: { "/v1/permissions/ai": anAiPermissionsStatus() } }, [LOCAL, DOWN])
  renderScreen(<PermissionsPage />, { route: "/permissions?tab=ai" })

  await screen.findByText("Host box is not reachable")
  expect(screen.getByText(REASON)).toBeTruthy()
  await screen.findByRole("heading", { name: /AI/ })
  expect(document.body.querySelector('[data-slot="skeleton"]')).toBeNull()
})

it("shows an error state, with the host's reason and no URL hint, when a named host's goals fail", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  const failing = () => errorResponse(500, "boom", "box is broken")
  stubHosts({ local: {}, box: { "/v1/goals": failing, "/v1/tasks": [] } })
  renderScreen(<GoalsListPage />, { route: "/goals" })

  await screen.findByText("Could not load goals on box")
  expect(screen.getByText(/box is broken/)).toBeTruthy()
  expect(screen.queryByText(/Check the URL in settings/)).toBeNull()
})

it("warns, not errors, when only one of two hosts fails to list sessions", async () => {
  const failing = () => errorResponse(500, "boom", "box is broken")
  stubHosts({
    local: { "/v1/sessions": aSessionPage([aSession()]), "/v1/goals": [], "/v1/tasks": [] },
    box: { "/v1/sessions": failing, "/v1/goals": [], "/v1/tasks": [] },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  const title = await screen.findByText("Could not load sessions on box")
  const alert = title.closest('[role="alert"]') as HTMLElement
  expect(within(alert).getByText(/box is broken/)).toBeTruthy()
  expect(alert.className).not.toContain("destructive")
})

it("shows an error state for each host when every host fails to list sessions", async () => {
  const failing = () => errorResponse(500, "boom", "it is broken")
  stubHosts({
    local: { "/v1/sessions": failing, "/v1/goals": [], "/v1/tasks": [] },
    box: { "/v1/sessions": failing, "/v1/goals": [], "/v1/tasks": [] },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  await screen.findByText("Could not load sessions on local")
  expect(screen.getByText("Could not load sessions on box")).toBeTruthy()
  expect(screen.getAllByRole("alert")).toHaveLength(2)
})

it("lists the agents of each selected host in the sessions Agent filter", async () => {
  const user = userEvent.setup()
  const agent = (id: string) => ({ id })
  stubHosts({
    local: {
      "/v1/sessions": aSessionPage([]),
      "/v1/goals": [],
      "/v1/tasks": [],
      "/v1/acp-agents": [agent("agent-on-local")],
    },
    box: {
      "/v1/sessions": aSessionPage([]),
      "/v1/goals": [],
      "/v1/tasks": [],
      "/v1/acp-agents": [agent("agent-on-box")],
    },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  await user.click(await screen.findByRole("button", { name: "Filter by agent" }))
  expect(await screen.findByRole("menuitemradio", { name: "agent-on-box" })).toBeTruthy()
  expect(screen.getByRole("menuitemradio", { name: "agent-on-local" })).toBeTruthy()
})

function tableOf(host?: string, error: Error = new Error("no route to host")) {
  return (
    <DataTable
      query={{ data: undefined, isPending: false, isError: true, error, refetch: vi.fn() }}
      errorTitle="Could not load things"
      columns={[{ header: "Name" }]}
      empty="none"
      rowKey={() => "x"}
      renderRow={() => null}
      host={host}
    />
  )
}

it("shows the daemon hint only for local, on a table of a named host", () => {
  const network = ApiError.network(new Error("fetch failed"))
  const view = render(tableOf("box", network))
  expect(screen.getByText("Could not load things on box")).toBeTruthy()
  expect(screen.getByText(/fetch failed/)).toBeTruthy()
  expect(screen.queryByText(/Check the URL in settings/)).toBeNull()
  view.unmount()

  render(tableOf(undefined, network))
  expect(screen.getByText("Could not load things")).toBeTruthy()
  expect(screen.getByText(/Check the URL in settings/)).toBeTruthy()
})
