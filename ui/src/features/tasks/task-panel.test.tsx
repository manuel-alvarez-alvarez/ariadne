// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { qk } from "@/api"
import { aGoal, aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { TaskPanel } from "./task-panel"

it("shows workflow agents by column", async () => {
  const goal = aGoal({
    steps: [
      { id: "build", title: "Build", description: "Build the change.", skills: ["coding"] },
      { id: "test", title: "Test", description: "Test the change.", skills: ["testing"] },
    ],
  })
  const task = aTask({
    goal_id: goal.id,
    step: "build",
    agents: [
      { id: "agent-build", step: "build", skills: ["coding"], model: "codex-acp:o3" },
      { id: "agent-test", step: "test", skills: ["testing"], model: "codex-acp:o3" },
    ],
  })
  renderScreen(<TaskPanel taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail(task.id), task)
      client.setQueryData(qk.goals.detail(goal.id), goal)
    },
  })
  expect(await screen.findByText("Agents")).toBeTruthy()
  expect(screen.getAllByText("Build").length).toBeGreaterThan(0)
  expect(screen.getAllByText("Test").length).toBeGreaterThan(0)
})
