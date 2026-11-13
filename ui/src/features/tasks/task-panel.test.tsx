// @vitest-environment jsdom
import { screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import { qk } from "@/api"
import { aGoal, aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { TaskPanel } from "./task-panel"

it("shows workflow agents by column", async () => {
  const goal = aGoal({
    steps: [
      {
        id: "build",
        title: "Build",
        description: "Build the change.",
        skills: ["coding"],
        ranks: [],
      },
      {
        id: "test",
        title: "Test",
        description: "Test the change.",
        skills: ["testing"],
        ranks: [],
      },
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
  expect(await screen.findByText("Build")).toBeTruthy()
  expect(screen.getByText("Test")).toBeTruthy()
  expect(screen.queryByText("Agents")).toBeNull()
})

it("explains a pending task by the dependencies it waits for", async () => {
  const dependency = aTask({ id: "dep-1", title: "Lay the schema", status: "in_progress" })
  const task = aTask({ status: "pending", step: null, depends_on: [dependency.id] })
  renderScreen(<TaskPanel taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail(task.id), task)
      client.setQueryData(qk.tasks.detail(dependency.id), dependency)
    },
  })
  const summary = await screen.findByRole("region", { name: "Execution summary" })
  expect(summary.textContent).toContain("Waiting for Lay the schema (in progress).")
  expect(within(summary).getByRole("link", { name: "Lay the schema" })).toBeTruthy()
})

it("says the cause is unknown for a pending task with nothing recorded", async () => {
  const task = aTask({ status: "pending", step: null, depends_on: [] })
  renderScreen(<TaskPanel taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail(task.id), task),
  })
  const summary = await screen.findByRole("region", { name: "Execution summary" })
  expect(summary.textContent).toContain("The cause is unknown.")
})
