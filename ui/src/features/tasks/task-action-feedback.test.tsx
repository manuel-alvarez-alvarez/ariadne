// @vitest-environment jsdom
import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { qk, type TaskDto } from "@/api"
import { aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { TaskPanel } from "./task-panel"

const feedback = vi.hoisted(() => vi.fn())
vi.mock("sonner", () => ({ toast: { success: feedback, error: vi.fn() } }))

function remote(status: TaskDto["status"]) {
  return aTask({
    status,
    execution_context: {
      source_goal: { daemon_id: "source", goal_id: "goal", title: "Source", description: "" },
      execution_owner: "worker",
      workflow: { name: "saved", steps: [] },
      revision: 1,
      authorized: false,
      authorized_revision: null,
    },
  })
}

it.each([
  ["pending", "Cancellation requested", "Pending"],
  ["in_progress", "Cancellation requested", "In progress"],
  ["finished", "Task finished before cancellation", "Finished"],
] as const)("reports cancellation response %s", async (status, message, label) => {
  feedback.mockClear()
  const user = userEvent.setup()
  const task = remote("in_progress")
  daemonFetch.mockImplementation(async (request: Request) =>
    request.url.endsWith("/cancel") ? jsonResponse({ ...task, status }) : jsonResponse([]),
  )
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  await user.click(screen.getByRole("button", { name: "Cancel task" }))
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel task" }))
  await waitFor(() =>
    expect(feedback).toHaveBeenCalledWith(message, {
      description: status === "finished" ? "The task completed." : `Status: ${label}`,
    }),
  )
  expect(screen.getAllByText(label).length).toBeGreaterThan(0)
})

it.each([
  ["pending", "Pending"],
  ["ready", "Ready"],
  ["in_progress", "In progress"],
] as const)("reports retry response %s", async (status, label) => {
  feedback.mockClear()
  const user = userEvent.setup()
  const task = remote("failed")
  daemonFetch.mockImplementation(async (request: Request) =>
    request.url.endsWith("/retry") ? jsonResponse({ ...task, status }) : jsonResponse([]),
  )
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  await user.click(screen.getByRole("button", { name: "Retry task" }))
  await waitFor(() =>
    expect(feedback).toHaveBeenCalledWith("Task retried", {
      description: expect.stringContaining(label),
    }),
  )
  expect(screen.getAllByText(label).length).toBeGreaterThan(0)
})

it("reports a retry request awaiting worker acknowledgment", async () => {
  feedback.mockClear()
  const user = userEvent.setup()
  const task = remote("failed")
  daemonFetch.mockImplementation(async (request: Request) =>
    request.url.endsWith("/retry") ? jsonResponse(task) : jsonResponse([]),
  )
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  await user.click(screen.getByRole("button", { name: "Retry task" }))
  await waitFor(() =>
    expect(feedback).toHaveBeenCalledWith("Retry requested", {
      description: "The task remains failed until the worker acknowledges recovery.",
    }),
  )
})
