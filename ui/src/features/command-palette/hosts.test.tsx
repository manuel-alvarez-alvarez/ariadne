// @vitest-environment jsdom

/**
 * The palette across hosts: it searches the goals, tasks and sessions of every
 * reachable host, whatever the pick, each row opens on its host, and the attach command names a
 * host that is not `local`.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { useSettingsStore } from "@/stores/settings"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"
import { CommandPalette } from "./command-palette"

const copied = vi.hoisted(() => vi.fn())
vi.mock("@/components/copyable-id", async (original) => ({
  ...(await original<typeof import("@/components/copyable-id")>()),
  copyEntry: copied,
}))

const GOAL = aGoal({ id: "01JBOXGOAL000000000000000B", title: "Goal on box" })
const TASK = aTask({ id: "01JBOXTASK000000000000000T", goal_id: GOAL.id, title: "Zebra on box" })
const MINE = aTask({ id: "01JLOCTASK000000000000000L", goal_id: "g", title: "Zebra on local" })
const SESSION = aSession({ id: "01JBOXSESS00000000000000S1", goal_id: GOAL.id, task_id: TASK.id })

const handlers = {
  onOpenChange: vi.fn(),
  onOpenSettings: vi.fn(),
  onNewGoal: vi.fn(),
  onOpenLogs: vi.fn(),
  onOpenShortcuts: vi.fn(),
  onToggleSidebar: vi.fn(),
}

const EMPTY = { "/v1/attention": { items: [], complete: true }, "/v1/repositories": [] }

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ hostFilter: "" })
  copied.mockClear()
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [], "/v1/tasks": [MINE], "/v1/sessions": aSessionPage([]) },
    box: {
      ...EMPTY,
      "/v1/goals": [GOAL],
      "/v1/tasks": [TASK],
      "/v1/sessions": aSessionPage([SESSION]),
    },
  })
})
afterEach(() => forgetHosts())

it("lists the tasks of every selected host, and opens a row on its own host", async () => {
  const user = userEvent.setup()
  const { location } = renderScreen(<CommandPalette open {...handlers} />, { route: "/goals" })

  await user.type(screen.getByRole("combobox"), "zebra")

  const remote = await screen.findByRole("option", { name: /^Zebra on box/ })
  expect(remote.textContent).toContain("box")
  expect(screen.getByRole("option", { name: /^Zebra on local/ })).toBeTruthy()
  await user.click(remote)

  expect(location.url).toBe(`/goals?host=box&task=${TASK.id}`)
})

it("names local on a local row under All", async () => {
  const user = userEvent.setup()
  renderScreen(<CommandPalette open {...handlers} />, { route: "/goals" })

  await user.type(screen.getByRole("combobox"), "zebra")

  const mine = await screen.findByRole("option", { name: /^Zebra on local/ })
  expect(within(mine).getByTitle("Host").textContent).toBe("local")
  const remote = screen.getByRole("option", { name: /^Zebra on box/ })
  expect(within(remote).getByTitle("Host").textContent).toBe("box")
})

it("names the host of every row under one picked host", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  const user = userEvent.setup()
  renderScreen(<CommandPalette open {...handlers} />, { route: "/goals" })

  await user.type(screen.getByRole("combobox"), "zebra")

  const remote = await screen.findByRole("option", { name: /^Zebra on box/ })
  expect(within(remote).getByTitle("Host").textContent).toBe("box")
})

it("offers New task for the open goal on its own host under another pick", async () => {
  useSettingsStore.setState({ hostFilter: "local" })
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [], "/v1/tasks": [MINE], "/v1/sessions": aSessionPage([]) },
    box: { ...EMPTY, [`/v1/goals/${GOAL.id}`]: GOAL },
  })
  renderScreen(<CommandPalette open {...handlers} />, { route: `/goals?goal=${GOAL.id}&host=box` })

  const item = await screen.findByRole("option", { name: /^New task/ })
  expect(item.textContent).toContain(GOAL.title)
})

it("searches the hosts outside the pick too", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  const user = userEvent.setup()
  renderScreen(<CommandPalette open {...handlers} />, { route: "/goals" })

  await user.type(screen.getByRole("combobox"), "zebra")

  await screen.findByRole("option", { name: /^Zebra on box/ })
  const mine = await screen.findByRole("option", { name: /^Zebra on local/ })
  expect(within(mine).getByTitle("Host").textContent).toBe("local")
})

it("copies the attach command with the host of the open session", async () => {
  const user = userEvent.setup()
  renderScreen(<CommandPalette open {...handlers} />, {
    route: `/sessions?session=${SESSION.id}&host=box`,
  })

  await user.click(await screen.findByText("Copy attach command"))

  await waitFor(() => expect(copied).toHaveBeenCalled())
  expect(copied.mock.calls[0]?.[0].text).toBe(`ariadne attach ${SESSION.id} --host box`)
})

it("copies the attach command of a local task with no host flag", async () => {
  const user = userEvent.setup()
  renderScreen(<CommandPalette open {...handlers} />, { route: `/goals?task=${MINE.id}` })

  await user.click(await screen.findByText("Copy attach command"))

  await waitFor(() => expect(copied).toHaveBeenCalled())
  expect(copied.mock.calls[0]?.[0].text).toBe(`ariadne attach ${MINE.id}`)
})
