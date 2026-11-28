// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { qk } from "@/api"
import { SessionActions } from "@/features/sessions/session-actions"
import { TaskActions } from "@/features/tasks/task-actions"
import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { GoalActions } from "./goal-actions"
import { GoalPanel } from "./goal-panel"
import { GoalTasks } from "./goal-tasks"

function iconOnly(name: string) {
  const button = screen.getByRole("button", { name })
  expect(button.textContent).toBe("")
  expect(button.querySelector("svg")).not.toBeNull()
}

it("draws Complete goal and Cancel goal as icon-only buttons on an active goal", () => {
  renderScreen(<GoalActions host="local" goal={aGoal({ status: "active" })} />)

  iconOnly("Complete goal")
  iconOnly("Cancel goal")
  expect(screen.queryByRole("button", { name: "Delete goal" })).toBeNull()
})

it("draws Delete goal as an icon-only button on a stopped goal", () => {
  renderScreen(<GoalActions host="local" goal={aGoal({ status: "cancelled" })} />)

  iconOnly("Delete goal")
})

it("draws New task as an icon-only button in the goal panel header", async () => {
  const goal = aGoal({ status: "active" })
  daemonFetch.mockImplementation(async () => jsonResponse([]))
  renderScreen(<GoalPanel host="local" goalId={goal.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.goals.detail("local", goal.id), goal),
  })

  await screen.findByRole("button", { name: "New task" })
  iconOnly("New task")
})

it("draws New task as an icon-only button in the empty task list", async () => {
  daemonFetch.mockImplementation(async () => jsonResponse([]))
  renderScreen(<GoalTasks host="local" goalId={aGoal().id} steps={[]} onNewTask={() => {}} />)

  await screen.findByRole("button", { name: "New task" })
  iconOnly("New task")
})

it("draws Edit task and Cancel task as icon-only buttons on a task that waits", () => {
  renderScreen(<TaskActions task={aTask({ status: "ready" })} />)

  iconOnly("Edit task")
  iconOnly("Cancel task")
})

it("draws Resume session as an icon-only button on a session that ended", () => {
  renderScreen(<SessionActions host="local" session={aSession({ status: "exited" })} />)

  iconOnly("Resume session")
  expect(screen.queryByRole("button", { name: "Kill session" })).toBeNull()
})
