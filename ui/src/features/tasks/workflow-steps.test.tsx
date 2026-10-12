// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { WorkflowSteps } from "./workflow-steps"

const STEPS = [
  { id: "build", title: "Build", description: "Build the change.", skills: ["coding"] },
  { id: "test", title: "Test", description: "Test the change.", skills: ["testing"] },
]

it("lists every column's name and progress, without opening any of them", () => {
  const task = aTask({
    status: "in_progress",
    step: "build",
    agents: [
      { id: "agent-build", step: "build", skills: ["coding"], model: "codex-acp:o3" },
      { id: "agent-test", step: "test", skills: ["testing"], model: "codex-acp:o3" },
    ],
  })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  expect(screen.getByText("Build")).toBeTruthy()
  expect(screen.getByText("Test")).toBeTruthy()
  expect(screen.getByText("Current")).toBeTruthy()
  expect(screen.getByText("Pending")).toBeTruthy()
  expect(screen.queryByText("Build the change.")).toBeNull()
  expect(screen.queryByText("codex-acp:o3", { exact: false })).toBeNull()
})

it("opens one column's description, skills, model and session from the keyboard, leaving the rest closed", async () => {
  const task = aTask({
    status: "in_progress",
    step: "build",
    agents: [
      {
        id: "agent-build",
        step: "build",
        skills: ["coding"],
        model: "codex-acp:o3",
        session_id: "01JSESSION000000000000001",
      },
      { id: "agent-test", step: "test", skills: ["testing"], model: "codex-acp:o3" },
    ],
  })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  const user = userEvent.setup()
  const buildTrigger = screen.getByRole("button", { name: /Build/ })
  buildTrigger.focus()
  await user.keyboard("{Enter}")
  expect(await screen.findByText("Build the change.")).toBeTruthy()
  expect(screen.getByText("coding")).toBeTruthy()
  expect(screen.getByTitle("codex-acp:o3")).toBeTruthy()
  expect(screen.getByRole("link", { name: /session/ })).toBeTruthy()
  expect(screen.queryByText("Test the change.")).toBeNull()
  expect(screen.queryByText("testing")).toBeNull()
})

it("marks every column pending for a task that has not started, in the pending colour", () => {
  const task = aTask({ status: "pending", step: null, agents: [] })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  const pending = screen.getAllByText("Pending")
  expect(pending).toHaveLength(2)
  for (const badge of pending) expect(badge.className).toContain("text-muted-foreground")
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
  expect(screen.getByText("Done").className).toContain("text-status-done-fg")
  expect(screen.getByText("Current").className).toContain("text-status-active-fg")
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
  const done = screen.getAllByText("Done")
  expect(done).toHaveLength(2)
  for (const badge of done) expect(badge.className).toContain("text-status-done-fg")
})
