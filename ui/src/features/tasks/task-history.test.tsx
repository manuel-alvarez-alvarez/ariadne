// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { qk, type TaskTransitionDto } from "@/api"
import { stubEventSource } from "@/test/event-source"
import { aGoal, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { TaskPanel } from "./task-panel"

const TASK = aTask()

function aTransition(overrides: Partial<TaskTransitionDto> = {}): TaskTransitionDto {
  return {
    id: "01JTRANSITION000000000001",
    actor: "agent",
    from_status: "ready",
    to_status: "in_progress",
    reason: null,
    created_at: "2026-01-01T00:00:00Z",
    ...overrides,
  }
}

function stubTransitions(transitions: TaskTransitionDto[]): void {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (
      request.method === "GET" &&
      new URL(request.url).pathname === `/v1/tasks/${TASK.id}/transitions`
    ) {
      return Promise.resolve(jsonResponse(transitions))
    }
    return new Promise(() => {})
  })
}

function renderHistory(): void {
  const goal = aGoal({ id: TASK.goal_id })
  renderScreen(<TaskPanel taskId={TASK.id} onClose={() => {}} />, {
    route: "/goals?tab=history",
    seed: (client) => {
      client.setQueryData(qk.tasks.detail(TASK.id), TASK)
      client.setQueryData(qk.goals.detail(goal.id), goal)
    },
  })
}

beforeEach(() => stubEventSource())

it("renders a transition's reason as Markdown", async () => {
  stubTransitions([
    aTransition({
      reason: "Changes requested:\n\n- run `cargo fmt`\n- see [the guide](https://example.com)",
    }),
  ])

  renderHistory()

  const code = await screen.findByText("cargo fmt")
  expect(code.tagName).toBe("CODE")
  expect(code.closest("li")?.parentElement?.tagName).toBe("UL")
  const link = screen.getByRole("link", { name: "the guide" })
  expect(link.getAttribute("href")).toBe("https://example.com")
  expect(link.getAttribute("target")).toBe("_blank")
  expect(screen.queryByText(/`cargo fmt`/)).toBeNull()
})
