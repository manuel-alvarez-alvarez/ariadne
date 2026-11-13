// @vitest-environment jsdom

/**
 * A refresh that fails with content cached keeps that content, empty lists
 * included, under one retryable "Could not refresh" warning (spec 015).
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { DataTable } from "@/components/data-table"
import { GoalTasks } from "@/features/goals/goal-tasks"
import { stubEventSource } from "@/test/event-source"
import { aGoal, aTask } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"

import { taskListQueryOptions } from "./queries"
import { TaskPanel } from "./task-panel"

const TASK = aTask()

beforeEach(() => {
  stubEventSource()
})

function failEveryRead(): void {
  daemonFetch.mockImplementation(() =>
    Promise.resolve(errorResponse(503, "unavailable", "daemon is busy")),
  )
}

function answerEveryRead(body: unknown): void {
  daemonFetch.mockImplementation(() => Promise.resolve(jsonResponse(body)))
}

it("keeps an empty goal task list under the warning, and clears it on a successful retry", async () => {
  const user = userEvent.setup()
  const { queryClient } = renderScreen(<GoalTasks goalId={TASK.goal_id} steps={[]} />, {
    seed: (client) =>
      client.setQueryData(taskListQueryOptions({ goal: TASK.goal_id }).queryKey, []),
  })
  await screen.findByText("No tasks yet")

  failEveryRead()
  await queryClient.refetchQueries({ queryKey: qk.tasks.lists() })
  await screen.findByText("Could not refresh tasks")
  expect(screen.getByText("No tasks yet")).toBeTruthy()

  answerEveryRead([])
  await user.click(screen.getByRole("button", { name: "Retry" }))
  await waitFor(() => expect(screen.queryByText("Could not refresh tasks")).toBeNull())
  expect(screen.getByText("No tasks yet")).toBeTruthy()
})

it.each([
  ["history", "The task has not moved yet"],
  ["messages", "The agents have said nothing yet"],
])(
  "keeps the cached task and its empty %s under warnings, and a retry clears them",
  async (tab, empty) => {
    const user = userEvent.setup()
    const goal = aGoal({ id: TASK.goal_id })
    const { queryClient } = renderScreen(<TaskPanel taskId={TASK.id} onClose={() => {}} />, {
      route: `/goals?tab=${tab}`,
      seed: (client) => {
        client.setQueryData(qk.tasks.detail(TASK.id), TASK)
        client.setQueryData(qk.goals.detail(goal.id), goal)
        client.setQueryData(qk.tasks.transitions(TASK.id), [])
        client.setQueryData(qk.tasks.messages(TASK.id), [])
      },
    })
    await screen.findByText(empty)

    failEveryRead()
    await queryClient.refetchQueries({ queryKey: qk.tasks.detail(TASK.id) })

    await screen.findByText("Could not refresh this task")
    await screen.findByText(`Could not refresh ${tab}`)
    expect(screen.getByText(empty)).toBeTruthy()
    expect(screen.queryByText(/Could not load/)).toBeNull()

    daemonFetch.mockImplementation((input: Request | string | URL) => {
      const url = new URL(input instanceof Request ? input.url : String(input))
      return Promise.resolve(jsonResponse(url.pathname === `/v1/tasks/${TASK.id}` ? TASK : []))
    })
    for (const retry of screen.getAllByRole("button", { name: "Retry" })) await user.click(retry)
    await waitFor(() => expect(screen.queryByText(/Could not refresh/)).toBeNull())
    expect(screen.getByText(empty)).toBeTruthy()
  },
)

it("keeps cached table rows under the warning, and shows a load error with none", async () => {
  const query = (over: object) => ({
    data: ["alpha"] as string[] | undefined,
    isPending: false,
    isError: true,
    error: new Error("boom"),
    refetch: () => {},
    ...over,
  })
  const table = (q: ReturnType<typeof query>) => (
    <DataTable
      query={q}
      errorTitle="Could not load profiles"
      columns={[{ header: "Name" }]}
      empty="none"
      rowKey={(row) => row}
      renderRow={(row) => (
        <tr>
          <td>{row}</td>
        </tr>
      )}
    />
  )
  const view = renderScreen(table(query({})), { route: null })
  expect(screen.getByText("Could not refresh profiles")).toBeTruthy()
  expect(screen.getByText("alpha")).toBeTruthy()

  view.rerender(table(query({ data: [] })))
  expect(screen.getByText("Could not refresh profiles")).toBeTruthy()
  expect(screen.getByText("none")).toBeTruthy()

  view.rerender(table(query({ data: undefined })))
  expect(screen.getByText("Could not load profiles")).toBeTruthy()
  expect(screen.queryByText("Could not refresh profiles")).toBeNull()

  view.rerender(table(query({ isError: false, error: null })))
  expect(screen.queryByText("Could not refresh profiles")).toBeNull()
  expect(screen.getByText("alpha")).toBeTruthy()
})
