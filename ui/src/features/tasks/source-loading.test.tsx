// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import { expect, it } from "vitest"
import { qk } from "@/api"
import { SessionDetailView } from "@/features/sessions/session-detail-view"
import { useSettingsStore } from "@/stores/settings"
import { aSession, aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { stubHosts } from "@/test/hosts"
import { TaskCard } from "./task-card"
import { TaskPanel } from "./task-panel"
import { TaskRow } from "./task-row"

it.each([TaskCard, TaskRow])("shows a skeleton while the worker identity loads: %s", (View) => {
  stubHosts({})
  const task = aTask({
    execution_context: {
      execution_owner: "worker",
      source_goal: { daemon_id: "source", goal_id: "goal", title: "Source", description: "" },
      workflow: { name: "saved", steps: [] },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  renderScreen(<View host="local" task={task} steps={[]} />)
  expect(document.querySelector('[aria-label="Loading worker"][data-slot="skeleton"]')).toBeTruthy()
  expect(screen.queryByText("Unconfigured worker")).toBeNull()
})

it("shows a skeleton while the task goal loads", () => {
  stubHosts({})
  const task = aTask()
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  expect(document.querySelector('[aria-label="Loading goal"][data-slot="skeleton"]')).toBeTruthy()
  expect(screen.queryByRole("button", { name: "Goal" })).toBeNull()
})

it.each(["panel", "session"])("shows a skeleton while the source loads in the %s", (view) => {
  stubHosts({})
  useSettingsStore.setState({ sessionFactsOpen: true })
  const task = aTask({
    execution_context: {
      execution_owner: "worker",
      source_goal: { daemon_id: "source", goal_id: "goal", title: "Source", description: "" },
      workflow: { name: "saved", steps: [] },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  renderScreen(
    view === "panel" ? (
      <TaskPanel host="local" taskId={task.id} onClose={() => {}} />
    ) : (
      <SessionDetailView
        host="local"
        session={aSession({ task_id: task.id, goal_id: task.goal_id })}
      />
    ),
    {
      seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
    },
  )
  expect(document.querySelector('[aria-label="Loading source"][data-slot="skeleton"]')).toBeTruthy()
  expect(screen.queryByText(/source.*unavailable/)).toBeNull()
})
