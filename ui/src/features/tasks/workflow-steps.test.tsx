// @vitest-environment jsdom

import { screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import { aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { WorkflowSteps } from "./workflow-steps"

const STEPS = [
  { id: "build", title: "Build", description: "Build the change.", skills: ["coding"] },
  { id: "test", title: "Test", description: "Test the change.", skills: ["testing"] },
]

it("draws every column on the track with its agent's skills, model and session under it", () => {
  const task = aTask({
    status: "in_progress",
    step: "build",
    agents: [
      { id: "agent-build", step: "build", skills: ["coding"], model: "codex-acp:o3" },
      {
        id: "agent-test",
        step: "test",
        skills: ["testing"],
        model: "codex-acp:o4",
        session_id: "01JSESSION000000000000001",
      },
    ],
  })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  const [build, test] = screen.getAllByRole("listitem")
  if (!build || !test) throw new Error("expected two columns")
  expect(build.getAttribute("aria-current")).toBe("step")
  expect(within(build).getByText("Build")).toBeTruthy()
  expect(within(build).getByText("coding")).toBeTruthy()
  expect(within(build).getByTitle("codex-acp:o3")).toBeTruthy()
  expect(within(build).queryByRole("link", { name: /session/ })).toBeNull()
  expect(test.getAttribute("aria-current")).toBeNull()
  expect(within(test).getByText("Test")).toBeTruthy()
  expect(within(test).getByText("testing")).toBeTruthy()
  expect(within(test).getByTitle("codex-acp:o4")).toBeTruthy()
  expect(within(test).getByRole("link", { name: /session/ })).toBeTruthy()
  // The description belongs to the workflow document, not this track.
  expect(screen.queryByText("Build the change.")).toBeNull()
  expect(screen.queryByText("Test the change.")).toBeNull()
})

it("marks every column pending for a task that has not started, in the pending colour", () => {
  const task = aTask({ status: "pending", step: null, agents: [] })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  const columns = screen.getAllByRole("listitem")
  expect(columns).toHaveLength(2)
  for (const column of columns) {
    expect(column.getAttribute("data-step-state")).toBe("ahead")
    expect(within(column).getByText("Pending")).toBeTruthy()
    expect(within(column).getByText("none staffed")).toBeTruthy()
  }
})

it("marks the columns behind the current one done, and the rest ahead, in their own colours", () => {
  const task = aTask({
    status: "in_progress",
    step: "test",
    agents: [
      { id: "agent-build", step: "build", skills: ["coding"], model: "codex-acp:o3" },
      { id: "agent-test", step: "test", skills: ["testing"], model: "codex-acp:o3" },
    ],
  })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  const [build, test] = screen.getAllByRole("listitem")
  if (!build || !test) throw new Error("expected two columns")
  expect(build.getAttribute("data-step-state")).toBe("done")
  expect(within(build).getByText("Done")).toBeTruthy()
  expect(test.getAttribute("data-step-state")).toBe("current")
  expect(within(test).getByText("Current")).toBeTruthy()
})

it("marks every column done for a finished task, in the completion colour", () => {
  const task = aTask({
    status: "finished",
    step: "test",
    agents: [
      { id: "agent-build", step: "build", skills: ["coding"], model: "codex-acp:o3" },
      { id: "agent-test", step: "test", skills: ["testing"], model: "codex-acp:o3" },
    ],
  })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  for (const column of screen.getAllByRole("listitem")) {
    expect(column.getAttribute("data-step-state")).toBe("done")
    expect(within(column).getByText("Done")).toBeTruthy()
  }
})
