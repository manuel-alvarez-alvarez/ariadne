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
