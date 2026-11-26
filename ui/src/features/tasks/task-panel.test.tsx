// @vitest-environment jsdom
import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, onTestFinished, vi } from "vitest"

import { qk } from "@/api"
import { EventStreamProvider } from "@/events/provider"
import { latestSource, stubEventSource } from "@/test/event-source"
import { aGoal, aSession, aSessionPage, aTask, GOAL_ID } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { TaskPanel } from "./task-panel"

/** The paths of every request the panel made, in order. */
function requestedPaths(): string[] {
  return daemonFetch.mock.calls.map(([input]) => {
    const url = input instanceof Request ? input.url : String(input)
    return new URL(url).pathname
  })
}

it("offers no messages tab and no message input, and reads no message route", async () => {
  const task = aTask()
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    route: "/goals?tab=messages",
    seed: (client) => {
      client.setQueryData(qk.tasks.detail("local", task.id), task)
      client.setQueryData(qk.goals.detail("local", GOAL_ID), aGoal({ id: GOAL_ID }))
    },
  })

  const tabs = await screen.findAllByRole("tab")
  expect(tabs.map((tab) => tab.textContent)).toEqual(["Description", "History", "Diff", "Sessions"])
  expect(screen.getByRole("tab", { name: "Description" }).getAttribute("aria-selected")).toBe(
    "true",
  )
  expect(screen.queryByRole("textbox")).toBeNull()
  expect(requestedPaths().filter((path) => path.endsWith("/messages"))).toEqual([])
})

it("changes no view and logs no error when the stream carries a message_sent event", async () => {
  stubEventSource()
  const warn = vi.spyOn(console, "warn")
  const error = vi.spyOn(console, "error")
  onTestFinished(() => {
    warn.mockRestore()
    error.mockRestore()
  })
  const task = aTask()
  // Every list read settles, so a refetch the event caused would be a new
  // request. The seeded task and goal stay as they are.
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const path = new URL(input instanceof Request ? input.url : String(input)).pathname
    if (path === `/v1/tasks/${task.id}` || path === `/v1/goals/${task.goal_id}`) {
      return new Promise(() => {})
    }
    return Promise.resolve(jsonResponse(path === "/v1/sessions" ? aSessionPage([]) : []))
  })
  renderScreen(
    <EventStreamProvider>
      <TaskPanel host="local" taskId={task.id} onClose={() => {}} />
    </EventStreamProvider>,
    {
      seed: (client) => {
        client.setQueryData(qk.tasks.detail("local", task.id), task)
        client.setQueryData(qk.goals.detail("local", GOAL_ID), aGoal({ id: GOAL_ID }))
      },
    },
  )
  await screen.findByRole("heading", { name: task.title })
  await waitFor(() =>
    expect(screen.getByRole("tab", { name: /Sessions/ }).textContent).toMatch(/0/),
  )
  const stream = latestSource()
  act(() => {
    stream.succeed()
    stream.beat()
  })
  const before = { html: document.body.innerHTML, requests: requestedPaths() }

  act(() =>
    stream.emit("message_sent", {
      id: "01JMESSAGE0000000000000001",
      goal_id: GOAL_ID,
      task_id: task.id,
      body: "Said by an agent.",
      created_at: "2026-01-01T00:00:00Z",
    }),
  )
  await act(async () => {})

  expect(document.body.innerHTML).toBe(before.html)
  expect(requestedPaths()).toEqual(before.requests)
  expect(warn).not.toHaveBeenCalled()
  expect(error).not.toHaveBeenCalled()
})

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
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail("local", task.id), task)
      client.setQueryData(qk.goals.detail("local", goal.id), goal)
    },
  })
  expect(await screen.findByText("Build")).toBeTruthy()
  expect(screen.getByText("Test")).toBeTruthy()
  expect(screen.queryByText("Agents")).toBeNull()
})

it("explains a pending task by the dependencies it waits for", async () => {
  const dependency = aTask({ id: "dep-1", title: "Lay the schema", status: "in_progress" })
  const task = aTask({ status: "pending", step: null, depends_on: [dependency.id] })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail("local", task.id), task)
      client.setQueryData(qk.tasks.detail("local", dependency.id), dependency)
    },
  })
  const summary = await screen.findByRole("region", { name: "Execution summary" })
  expect(summary.textContent).toContain("Waiting for a dependency")
  expect(within(summary).getByRole("listitem").textContent).toContain("In progress")
  expect(within(summary).getByRole("link", { name: "Lay the schema" })).toBeTruthy()
})

it("says the cause is unknown for a pending task with nothing recorded", async () => {
  const task = aTask({ status: "pending", step: null, depends_on: [] })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  const summary = await screen.findByRole("region", { name: "Execution summary" })
  expect(summary.textContent).toContain("Cause unknown")
})

it("does not claim a blocker while a dependency's status is unresolved", async () => {
  const task = aTask({ status: "pending", step: null, depends_on: ["dep-unloaded"] })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  const summary = await screen.findByRole("region", { name: "Execution summary" })
  expect(summary.textContent).not.toContain("Waiting for")
  expect(summary.textContent).toContain("Cause unknown")
})

it("lists unfinished dependencies of a failed task with its reason", async () => {
  const dependency = aTask({ id: "dep-2", title: "Lay the schema", status: "pending" })
  const task = aTask({
    status: "failed",
    step: null,
    reason: "dependency never landed",
    depends_on: [dependency.id],
  })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail("local", task.id), task)
      client.setQueryData(qk.tasks.detail("local", dependency.id), dependency)
    },
  })
  const summary = await screen.findByRole("region", { name: "Execution summary" })
  expect(summary.textContent).toContain("dependency never landed")
  const row = within(summary).getByRole("listitem")
  expect(row.textContent).toContain("Unfinished dependency")
  expect(row.textContent).toContain("Pending")
  expect(within(summary).getByRole("link", { name: "Lay the schema" })).toBeTruthy()
})

it("links a live session that waits for permission", async () => {
  const task = aTask({ status: "in_progress", step: null })
  const session = aSession({ task_id: task.id, attention_reason: "waiting_permission" })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail("local", task.id), task)
      client.setQueryData(qk.sessions.list("local", { task: task.id }), [session])
    },
  })
  const summary = await screen.findByRole("region", { name: "Execution summary" })
  expect(summary.textContent).toContain("A session is waiting for permission.")
  expect(within(summary).getByRole("link", { name: "Open the session" })).toBeTruthy()
})

it("shows the saved source and workflow when the worker has no goal", async () => {
  const task = aTask({
    goal_id: "source-goal",
    step: "build",
    execution_context: {
      source_goal: {
        daemon_id: "source",
        goal_id: "source-goal",
        title: "Source goal",
        description: "",
      },
      execution_owner: "worker",
      workflow: {
        name: "saved",
        steps: [{ id: "build", title: "Saved build", description: "", skills: [], ranks: [] }],
      },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  expect(await screen.findByText(/Source goal.*source.*unavailable/)).toBeTruthy()
  expect(screen.getByText("Saved build")).toBeTruthy()
})

it("links a projection to its source goal", async () => {
  const goal = aGoal({ title: "Source project" })
  const task = aTask({
    goal_id: goal.id,
    execution_context: {
      source_goal: { daemon_id: "source", goal_id: goal.id, title: goal.title, description: "" },
      execution_owner: "worker",
      workflow: { name: "saved", steps: [] },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  daemonFetch.mockImplementation(async (request) => {
    if (String(request.url).includes("/v1/remote-tasks/protocol"))
      return jsonResponse({ protocol: "remote-tasks-v1", daemon_id: "source" })
    return jsonResponse([])
  })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  expect(await screen.findByRole("button", { name: "Source project" })).toBeTruthy()
})

it("marks cached remote task progress as stale", async () => {
  const task = aTask({ host: "worker", host_unreachable: true })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  expect(await screen.findByText(/progress may be stale/i)).toBeTruthy()
})

it("keeps remote task progress while cancellation is pending", async () => {
  const user = userEvent.setup()
  const task = aTask({
    status: "in_progress",
    host: "worker",
    execution_context: {
      source_goal: { daemon_id: "source", goal_id: "goal", title: "Source", description: "" },
      execution_owner: "worker",
      workflow: { name: "saved", steps: [] },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  let requested = false
  daemonFetch.mockImplementation(async (request) => {
    if (request.url.endsWith("/cancel")) {
      requested = true
      return new Promise<Response>(() => {})
    }
    return jsonResponse([])
  })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  await user.click(screen.getByRole("button", { name: "Cancel task" }))
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel task" }))
  await waitFor(() => expect(requested).toBe(true))
  expect(screen.getAllByText("Cancelled")).toHaveLength(1)
})

it("names the worker in a source task panel", async () => {
  const task = aTask({ host: "worker" })
  renderScreen(<TaskPanel host="local" taskId={task.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.tasks.detail("local", task.id), task),
  })
  expect(await screen.findByText("worker")).toBeTruthy()
})
