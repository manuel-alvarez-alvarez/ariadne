// @vitest-environment jsdom

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { WorkflowSteps } from "./workflow-steps"

const STEPS = [
  { id: "build", title: "Build", description: "Build the change.", skills: ["coding"] },
  { id: "test", title: "Test", description: "Test the change.", skills: ["testing"] },
]

it("draws every column on the track and opens on the current one", () => {
  const task = aTask({
    status: "in_progress",
    step: "build",
    agents: [
      { id: "agent-build", step: "build", skills: ["coding"], model: "codex-acp:o3" },
      { id: "agent-test", step: "test", skills: ["testing"], model: "codex-acp:o3" },
    ],
  })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  const tabs = screen.getAllByRole("tab")
  expect(tabs.map((tab) => tab.textContent)).toEqual(["BuildCurrent", "TestPending"])
  expect(screen.getByRole("tab", { name: /Build/ }).getAttribute("aria-current")).toBe("step")
  expect(screen.getByRole("tab", { name: /Test/ }).getAttribute("aria-current")).toBeNull()
  // The current column's card is the one open, the other column's stays shut.
  expect(screen.getByText("Build the change.")).toBeTruthy()
  expect(screen.getByText("coding")).toBeTruthy()
  expect(screen.queryByText("Test the change.")).toBeNull()
  expect(screen.queryByText("testing")).toBeNull()
})

it("picks another column from the keyboard, showing its description, skills, model and session in place of the last", async () => {
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
  const user = userEvent.setup()
  screen.getByRole("tab", { name: /Build/ }).focus()
  await user.keyboard("{ArrowRight}")
  const card = screen.getByRole("tabpanel")
  expect(within(card).getByText("Test the change.")).toBeTruthy()
  expect(within(card).getByText("testing")).toBeTruthy()
  expect(within(card).getByTitle("codex-acp:o4")).toBeTruthy()
  expect(within(card).getByRole("link", { name: /session/ })).toBeTruthy()
  expect(screen.queryByText("Build the change.")).toBeNull()
  expect(screen.queryByText("coding")).toBeNull()
})

it("marks every column pending for a task that has not started, in the pending colour", () => {
  const task = aTask({ status: "pending", step: null, agents: [] })
  renderScreen(<WorkflowSteps task={task} steps={STEPS} />)
  for (const tab of screen.getAllByRole("tab")) {
    expect(tab.getAttribute("data-step-state")).toBe("ahead")
    expect(within(tab).getByText("Pending")).toBeTruthy()
  }
  const card = screen.getByRole("tabpanel")
  expect(within(card).getByText("Pending").className).toContain("text-muted-foreground")
  expect(within(card).getByText("none staffed")).toBeTruthy()
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
  const build = screen.getByRole("tab", { name: /Build/ })
  expect(build.getAttribute("data-step-state")).toBe("done")
  expect(within(build).getByText("Done")).toBeTruthy()
  expect(screen.getByRole("tab", { name: /Test/ }).getAttribute("data-step-state")).toBe("current")
  expect(within(screen.getByRole("tabpanel")).getByText("Current").className).toContain(
    "text-status-active-fg",
  )
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
  for (const tab of screen.getAllByRole("tab")) {
    expect(tab.getAttribute("data-step-state")).toBe("done")
    expect(within(tab).getByText("Done")).toBeTruthy()
  }
  expect(within(screen.getByRole("tabpanel")).getByText("Done").className).toContain(
    "text-status-done-fg",
  )
})
