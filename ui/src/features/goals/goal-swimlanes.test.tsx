// @vitest-environment jsdom
import { screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import { aGoal, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { GoalSwimlanes } from "./goal-swimlanes"

const GOAL_TITLE = "Ship the board"

it("shows all tasks in one area and counts them", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  daemonFetch.mockResolvedValue(
    jsonResponse([
      aTask({ id: "one", goal_id: goal.id, title: "First", status: "pending" }),
      aTask({ id: "two", goal_id: goal.id, title: "Second", status: "finished" }),
    ]),
  )
  renderScreen(<GoalSwimlanes goals={[goal]} />)
  expect(await screen.findByRole("heading", { name: "Tasks (2)" })).toBeTruthy()
  const area = screen.getByRole("group", { name: `${GOAL_TITLE}: Tasks` })
  expect(within(area).getByText("First")).toBeTruthy()
  expect(within(area).getByText("Second")).toBeTruthy()
})

it("hides terminal tasks under Active while preserving progress", async () => {
  const goal = aGoal({ title: GOAL_TITLE })
  daemonFetch.mockResolvedValue(
    jsonResponse([
      aTask({ id: "one", goal_id: goal.id, title: "Waiting", status: "pending" }),
      aTask({ id: "two", goal_id: goal.id, title: "Complete", status: "finished" }),
      aTask({ id: "three", goal_id: goal.id, title: "Cancelled task", status: "cancelled" }),
    ]),
  )
  renderScreen(<GoalSwimlanes goals={[goal]} activeOnly />)
  expect(await screen.findByRole("heading", { name: "Tasks (1)" })).toBeTruthy()
  expect(screen.getByText("Waiting")).toBeTruthy()
  expect(screen.queryByText("Complete")).toBeNull()
  expect(screen.queryByText("Cancelled task")).toBeNull()
  expect(screen.getByText("1/2")).toBeTruthy()
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
  expect(screen.queryByRole("group", { name: `${GOAL_TITLE}: Tasks` })).toBeNull()
})
