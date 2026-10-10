// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { aGoal, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { GoalSwimlanes } from "./goal-swimlanes"

it("draws each goal workflow columns", async () => {
  const goal = aGoal({
    steps: [
      { id: "build", title: "Build", description: "", skills: ["coding"] },
      { id: "test", title: "Test", description: "", skills: ["testing"] },
    ],
  })
  daemonFetch.mockResolvedValue(jsonResponse([aTask({ goal_id: goal.id, step: "build" })]))
  renderScreen(<GoalSwimlanes goals={[goal]} />)
  expect(await screen.findByText("Pending")).toBeTruthy()
  expect(screen.getByText("Build")).toBeTruthy()
  expect(screen.getByText("Test")).toBeTruthy()
  expect(screen.getByText("Done")).toBeTruthy()
})

it("draws an in-progress task of a stepless goal in its own column", async () => {
  const goal = aGoal({ steps: [] })
  daemonFetch.mockResolvedValue(jsonResponse([aTask({ goal_id: goal.id, status: "in_progress" })]))
  renderScreen(<GoalSwimlanes goals={[goal]} />)
  expect(await screen.findByText("Pending")).toBeTruthy()
  expect(screen.getByText("Done")).toBeTruthy()
  const column = screen.getByText("In progress")
  expect(column).toBeTruthy()
  expect(screen.getByText("Wire the sessions screen")).toBeTruthy()
})
