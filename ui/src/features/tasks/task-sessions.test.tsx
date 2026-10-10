// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import { expect, it, vi } from "vitest"

import { shortId } from "@/lib/format"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { TaskSessions } from "./task-sessions"

it("names a session row by the title of its agent's workflow step", async () => {
  const goal = aGoal({
    steps: [{ id: "build", title: "Build", description: "Build the change.", skills: ["coding"] }],
  })
  const task = aTask({
    goal_id: goal.id,
    agents: [{ id: "agent-build", step: "build", skills: ["coding"], model: "codex-acp:o3" }],
  })
  const staffed = aSession({ task_id: task.id, task_agent_id: "agent-build" })
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (request.method === "GET" && new URL(request.url).pathname === "/v1/sessions") {
      return Promise.resolve(jsonResponse(aSessionPage([staffed])))
    }
    return Promise.resolve(jsonResponse([]))
  })

  renderScreen(<TaskSessions task={task} steps={goal.steps} onSelect={vi.fn()} />, {
    route: "/goals",
  })

  expect(await screen.findByText("Build")).toBeTruthy()
})

it("names no step for a session with no staffed agent", async () => {
  const task = aTask({ agents: [] })
  const loose = aSession({ task_id: task.id, task_agent_id: null })
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (request.method === "GET" && new URL(request.url).pathname === "/v1/sessions") {
      return Promise.resolve(jsonResponse(aSessionPage([loose])))
    }
    return Promise.resolve(jsonResponse([]))
  })

  renderScreen(<TaskSessions task={task} steps={[]} onSelect={vi.fn()} />, { route: "/goals" })

  const row = await screen.findByText(shortId(loose.id))
  expect(row.closest("tr")?.textContent).not.toMatch(/build|test/i)
})

it("falls back to the step id where the workflow no longer names its title", async () => {
  const task = aTask({
    agents: [{ id: "agent-build", step: "build", skills: ["coding"], model: "codex-acp:o3" }],
  })
  const staffed = aSession({ task_id: task.id, task_agent_id: "agent-build" })
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (request.method === "GET" && new URL(request.url).pathname === "/v1/sessions") {
      return Promise.resolve(jsonResponse(aSessionPage([staffed])))
    }
    return Promise.resolve(jsonResponse([]))
  })

  // No workflow steps at all, so the title lookup has nothing to answer with.
  renderScreen(<TaskSessions task={task} steps={[]} onSelect={vi.fn()} />, { route: "/goals" })

  expect(await screen.findByText("build")).toBeTruthy()
})
