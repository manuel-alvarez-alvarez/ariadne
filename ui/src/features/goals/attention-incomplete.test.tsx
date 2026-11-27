// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"
import type { AttentionItemDto, AttentionListDto, SessionDto, TaskDto } from "@/api"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { AttentionAlerts, AttentionBadge } from "./attention-alerts"
import { AttentionPopover } from "./attention-strip"
import { GoalBoard } from "./goal-board"

const goal = aGoal()
const task = aTask({ goal_id: goal.id, status: "in_progress" })
const permission = aSession({
  goal_id: goal.id,
  task_id: task.id,
  status: "running",
  attention_reason: "waiting_permission",
})
const item: AttentionItemDto = {
  id: `agent_request:waiting_permission:${permission.id}`,
  occurrence: "permission-1",
  producer: "agent_request",
  reason: "unknown",
  summary: "Permission approval needed",
  required_action: "Answer in the console.",
  since: "2026-01-01T00:00:00Z",
  affected: [{ kind: "session", id: permission.id, label: task.title }],
  target: { kind: "console", session_id: permission.id },
}
let sessions: SessionDto[]
let tasks: TaskDto[]
let attention: AttentionListDto
let unavailable: boolean

beforeEach(() => {
  sessions = [permission]
  tasks = [task]
  attention = { items: [], complete: false }
  unavailable = false
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    if (url.pathname.endsWith("/dismiss")) {
      attention = { items: [], complete: false }
      return Promise.resolve(new Response(null, { status: 204 }))
    }
    if (url.pathname === "/v1/attention")
      return Promise.resolve(
        unavailable ? errorResponse(503, "unavailable", "Try again") : jsonResponse(attention),
      )
    return Promise.resolve(
      jsonResponse(
        url.pathname === "/v1/goals"
          ? [goal]
          : url.pathname === "/v1/tasks"
            ? tasks
            : aSessionPage(sessions),
      ),
    )
  })
})

it.each(["dismissed permission", "dismissed failed task", "dead session"])(
  "does not reconstruct a %s during an incomplete read",
  async (cause) => {
    if (cause === "dismissed failed task") {
      sessions = []
      tasks = [{ ...task, status: "failed" }]
    } else if (cause === "dead session") {
      sessions = [{ ...permission, status: "exited" }]
    }
    const { queryClient } = renderScreen(
      <>
        <AttentionPopover />
        <AttentionAlerts />
        <AttentionBadge />
      </>,
    )
    await waitFor(() => expect(queryClient.isFetching()).toBe(0))

    expect(document.title).toBe("(!) Ariadne")
    expect(screen.getByLabelText("could not read what needs attention")).not.toBeNull()
    fireEvent.click(screen.getByRole("button", { name: "Needs attention, 0 items" }))
    expect(await screen.findByText(/Could not load attention/)).not.toBeNull()
    expect(screen.getByRole("button", { name: "Retry" })).not.toBeNull()
    expect(screen.queryByRole("link", { name: /Open console|Open task/ })).toBeNull()
  },
)

it.each([
  { name: "dismissed task session", taskId: task.id, status: "running" as const },
  { name: "dismissed goal session", taskId: null, status: "running" as const },
  { name: "dead task session", taskId: task.id, status: "exited" as const },
  { name: "dead goal session", taskId: null, status: "exited" as const },
])("shows no board badge for a $name during an incomplete read", async ({ taskId, status }) => {
  sessions = [{ ...permission, task_id: taskId, status }]
  const { queryClient } = renderScreen(
    <GoalBoard hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />,
  )
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))

  expect(screen.getByText(task.title)).not.toBeNull()
  expect(screen.queryByText("Waiting for permission")).toBeNull()
})

it("keeps a dismissed permission hidden when the dismissal refresh is incomplete", async () => {
  attention = { items: [item], complete: true }
  renderScreen(<AttentionPopover />)
  fireEvent.click(await screen.findByRole("button", { name: "Needs attention, 1 items" }))
  fireEvent.click(await screen.findByRole("button", { name: "Dismiss" }))

  expect(await screen.findByText(/Could not load attention/)).not.toBeNull()
  expect(screen.getByRole("button", { name: "Needs attention, 0 items" })).not.toBeNull()
  expect(screen.queryByRole("link", { name: "Open console" })).toBeNull()
  expect(sessions[0]?.attention_reason).toBe("waiting_permission")
})

it("retains known items and their board badges while reporting an incomplete read", async () => {
  attention = { items: [item], complete: false }
  renderScreen(
    <>
      <AttentionPopover />
      <GoalBoard hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />
    </>,
  )
  fireEvent.click(await screen.findByRole("button", { name: "Needs attention, 1 items" }))
  expect(await screen.findByText(/Some items could not load/)).not.toBeNull()
  // The popover row and the board's block below the goal line.
  expect(screen.getAllByText("Permission approval needed")).toHaveLength(2)
  expect(screen.getByText("Waiting for permission")).not.toBeNull()
})

it("does not reconstruct rows or board badges when the attention read fails", async () => {
  unavailable = true
  const { queryClient } = renderScreen(
    <>
      <AttentionPopover />
      <GoalBoard hosts={["local"]} goals={[goal].map((goal) => ({ ...goal, host: "local" }))} />
    </>,
  )
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
  expect(screen.getByRole("button", { name: "Needs attention, 0 items" })).not.toBeNull()
  expect(screen.queryByText("Waiting for permission")).toBeNull()
})
