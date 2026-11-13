// @vitest-environment jsdom
import { screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import type { TaskStatus } from "@/api"
import { aGoal, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { GoalSwimlanes } from "./goal-swimlanes"

const GOAL_TITLE = "Ship the board"

it("draws the two column names once, however many goals", async () => {
  const goals = [aGoal({ id: "goal-a", title: "Alpha" }), aGoal({ id: "goal-b", title: "Beta" })]
  daemonFetch.mockResolvedValue(jsonResponse([]))
  renderScreen(<GoalSwimlanes goals={goals} />)

  const headings = await screen.findAllByRole("heading", { level: 2 })
  expect(headings.map((heading) => heading.textContent)).toEqual(["In progress", "Done"])
  expect(screen.queryByText("Pending")).toBeNull()
  expect(screen.queryByText("Cancelled")).toBeNull()
})

it.each<[TaskStatus, string, string]>([
  ["pending", "In progress", "Done"],
  ["ready", "In progress", "Done"],
  ["in_progress", "In progress", "Done"],
  ["failed", "In progress", "Done"],
  ["finished", "Done", "In progress"],
  ["cancelled", "Done", "In progress"],
])("files a %s task under %s, not %s", async (status, column, other) => {
  const goal = aGoal({ title: GOAL_TITLE, steps: [] })
  daemonFetch.mockResolvedValue(jsonResponse([aTask({ goal_id: goal.id, status })]))
  renderScreen(<GoalSwimlanes goals={[goal]} />)

  const cell = await screen.findByRole("group", { name: `${GOAL_TITLE}: ${column}` })
  expect(within(cell).getByText("Wire the sessions screen")).toBeTruthy()
  const otherCell = screen.getByRole("group", { name: `${GOAL_TITLE}: ${other}` })
  expect(within(otherCell).queryByText("Wire the sessions screen")).toBeNull()
})

it("keeps a stepped goal's tasks in the same two columns", async () => {
  const goal = aGoal({
    title: GOAL_TITLE,
    steps: [{ id: "build", title: "Build", description: "", skills: ["coding"], ranks: [] }],
  })
  daemonFetch.mockResolvedValue(jsonResponse([aTask({ goal_id: goal.id, step: "build" })]))
  renderScreen(<GoalSwimlanes goals={[goal]} />)

  const cell = await screen.findByRole("group", { name: `${GOAL_TITLE}: In progress` })
  expect(within(cell).getByText("Wire the sessions screen")).toBeTruthy()
  expect(screen.queryByRole("heading", { name: "Build" })).toBeNull()
})

it("draws a goal as one line: toggle, title link, status, progress and tokens", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "active" })
  daemonFetch.mockResolvedValue(
    jsonResponse([
      aTask({ id: "t1", goal_id: goal.id, status: "finished" }),
      aTask({ id: "t2", goal_id: goal.id, status: "in_progress" }),
    ]),
  )
  renderScreen(<GoalSwimlanes goals={[goal]} />)

  const link = await screen.findByRole("link", { name: GOAL_TITLE })
  expect(link.getAttribute("href")).toContain(`goal=${goal.id}`)
  const line = link.closest("header") as HTMLElement
  expect(within(line).getByRole("button", { name: `Collapse ${GOAL_TITLE}` })).toBeTruthy()
  expect(within(line).getByText("Active")).toBeTruthy()
  expect(within(line).getByText("1/2")).toBeTruthy()
  expect(within(line).getByText(/out/)).toBeTruthy()
  expect(screen.queryByRole("progressbar")).toBeNull()
})

it("shows the lane summary in place of the cells once folded", async () => {
  const goal = aGoal({ title: GOAL_TITLE, status: "completed" })
  daemonFetch.mockResolvedValue(jsonResponse([aTask({ goal_id: goal.id, status: "finished" })]))
  renderScreen(<GoalSwimlanes goals={[goal]} />)

  expect(await screen.findByText("1 task finished")).toBeTruthy()
  expect(screen.queryByRole("group", { name: `${GOAL_TITLE}: Done` })).toBeNull()
})
