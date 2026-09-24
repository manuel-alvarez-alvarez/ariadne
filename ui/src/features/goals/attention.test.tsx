// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { renderHook, waitFor } from "@testing-library/react"
import { createElement, type ReactNode } from "react"
import { describe, expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { sessionAttention } from "@/features/sessions/session-display"
import { paths } from "@/routes/paths"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse } from "@/test/harness"

import { attentionTarget, taskAttentionReason, useAttention, useBoardAttention } from "./attention"

function renderAttention<T>(hook: () => T) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  return renderHook(hook, {
    wrapper: ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children),
  })
}

function stubLists({
  goals = [aGoal()] as GoalDto[],
  tasks = [] as TaskDto[],
  sessions = [] as SessionDto[],
  fails,
  pending,
}: {
  goals?: GoalDto[]
  tasks?: TaskDto[]
  sessions?: SessionDto[]
  fails?: string | "all"
  pending?: boolean
} = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    if (pending) return new Promise<Response>(() => {})
    if (fails === "all" || fails === url.pathname)
      return Promise.resolve(new Response("boom", { status: 500 }))
    const body =
      url.pathname === "/v1/goals"
        ? goals
        : url.pathname === "/v1/tasks"
          ? tasks
          : aSessionPage(sessions)
    return Promise.resolve(jsonResponse(body))
  })
}

describe("attention", () => {
  it("leaves normal and changes-requested tasks alone", () => {
    expect(taskAttentionReason(aTask({ status: "in_progress" }))).toBeNull()
    expect(taskAttentionReason(aTask({ status: "changes_requested" }))).toBeNull()
  })

  it("reports failed and stalled tasks, with failure first", () => {
    expect(taskAttentionReason(aTask({ status: "failed" }))).toBe("failed")
    expect(taskAttentionReason(aTask({ stalled: true }))).toBe("stalled")
    expect(taskAttentionReason(aTask({ status: "failed", stalled: true }))).toBe("failed")
  })

  it("keeps only the session reasons that need attention", () => {
    expect(sessionAttention(aSession({ attention_reason: null }))).toBeNull()
    expect(sessionAttention(aSession({ status: "failed", attention_reason: null }))).toBeNull()
    expect(sessionAttention(aSession({ attention_reason: "waiting_permission" }))).toBe(
      "waiting_permission",
    )
    expect(sessionAttention(aSession({ attention_reason: "waiting_input" }))).toBe("waiting_input")
    expect(sessionAttention(aSession({ attention_reason: "agent_error" }))).toBe("agent_error")
    expect(sessionAttention(aSession({ attention_reason: "disconnected" }))).toBe("disconnected")
    expect(sessionAttention(aSession({ attention_reason: "stalled" }))).toBe("stalled")
    expect(sessionAttention(aSession({ status: "failed", attention_reason: "agent_error" }))).toBe(
      "agent_error",
    )
    expect(sessionAttention(aSession({ attention_reason: "waiting_user" }))).toBeNull()
  })

  it("opens a permission prompt in its session terminal", () => {
    const session = aSession({ id: "s1", task_id: "t1", attention_reason: "waiting_permission" })
    expect(
      attentionTarget(
        {
          id: "t1",
          goalId: "g1",
          goal: undefined,
          at: "2026-01-01T00:00:00Z",
          taskId: "t1",
          task: aTask({ id: "t1" }),
          taskReason: null,
          session,
          sessionReason: "waiting_permission",
        },
        new URLSearchParams(),
        "/goals",
      ),
    ).toEqual({ search: "?session=s1&tab=terminal&focus=terminal" })
  })

  it("opens a task row on the board from the sessions screen", () => {
    expect(
      attentionTarget(
        {
          id: "t1",
          goalId: "g1",
          goal: undefined,
          at: "2026-01-01T00:00:00Z",
          taskId: "t1",
          task: aTask({ id: "t1", status: "failed" }),
          taskReason: "failed",
          session: undefined,
          sessionReason: null,
        },
        new URLSearchParams("goal=g1"),
        "/sessions",
      ),
    ).toEqual({ pathname: "/goals", search: "?task=t1" })
  })

  it("stacks a task row on the screen where it was answered", () => {
    const target = attentionTarget(
      {
        id: "t1",
        goalId: "g1",
        goal: undefined,
        at: "2026-01-01T00:00:00Z",
        taskId: "t1",
        task: aTask({ id: "t1", status: "failed" }),
        taskReason: "failed",
        session: undefined,
        sessionReason: null,
      },
      new URLSearchParams("status=active"),
      paths.goals(),
    )

    expect(target.pathname).toBeUndefined()
    expect(new URLSearchParams(target.search).get("task")).toBe("t1")
  })

  it("keeps sessions filters below a task-less session panel", () => {
    const session = aSession({ id: "s1", task_id: null, attention_reason: "disconnected" })
    const target = attentionTarget(
      {
        id: session.id,
        goalId: session.goal_id ?? "",
        goal: undefined,
        at: session.created_at ?? "",
        taskId: null,
        task: undefined,
        taskReason: null,
        session,
        sessionReason: "disconnected",
      },
      new URLSearchParams("goal=g1&task=t1"),
      paths.sessions(),
    )

    const params = new URLSearchParams(target.search)
    expect(params.get("session")).toBe(session.id)
    expect(params.get("goal")).toBe("g1")
    expect(params.get("task")).toBe("t1")
  })

  it("reports a pending list before the daemon answers", () => {
    stubLists({ pending: true })
    const { result } = renderAttention(useAttention)

    expect(result.current.isPending).toBe(true)
    expect(result.current.items).toEqual([])
    expect(result.current.error).toBeNull()
  })

  it("reports a total list failure without calling it partial", async () => {
    stubLists({ fails: "all" })
    const { result } = renderAttention(useAttention)

    await waitFor(() => expect(result.current.isPending).toBe(false))
    expect(result.current.error).not.toBeNull()
    expect(result.current.items).toEqual([])
    expect(result.current.partial).toBe(false)
  })

  it("keeps loaded rows and calls the result partial when one list fails", async () => {
    stubLists({ tasks: [aTask({ id: "t1", status: "failed" })], fails: "/v1/sessions" })
    const { result } = renderAttention(useAttention)

    await waitFor(() => expect(result.current.items).toHaveLength(1))
    expect(result.current.error).not.toBeNull()
    expect(result.current.partial).toBe(true)
  })

  it("folds a task and its newest blocked session into one hook row", async () => {
    const task = aTask({ id: "t1", status: "failed", updated_at: "2026-01-01T00:00:00Z" })
    const session = aSession({
      id: "s1",
      task_id: task.id,
      attention_reason: "waiting_permission",
      attention_since: "2026-01-02T00:00:00Z",
    })
    stubLists({ tasks: [task], sessions: [session] })

    const { result } = renderAttention(useAttention)

    await waitFor(() => expect(result.current.isPending).toBe(false))
    expect(result.current.items).toMatchObject([
      {
        id: task.id,
        taskReason: "failed",
        session: { id: session.id },
        sessionReason: "waiting_permission",
      },
    ])
  })

  it("orders rows by their most recent attention", async () => {
    stubLists({
      tasks: [
        aTask({ id: "old", status: "failed", updated_at: "2026-01-01T00:00:00Z" }),
        aTask({ id: "new", stalled: true, updated_at: "2026-01-03T00:00:00Z" }),
      ],
      sessions: [
        aSession({
          id: "middle",
          task_id: null,
          attention_reason: "disconnected",
          ended_at: "2026-01-02T00:00:00Z",
        }),
      ],
    })
    const { result } = renderAttention(useAttention)

    await waitFor(() => expect(result.current.items).toHaveLength(3))
    expect(result.current.items.map((item) => item.id)).toEqual(["new", "middle", "old"])
  })

  it("keeps task and orchestrator rows when their goals are missing", async () => {
    const task = aTask({ id: "t1", goal_id: "missing-task-goal", status: "failed" })
    const orchestrator = aSession({
      id: "s1",
      goal_id: "missing-session-goal",
      task_id: null,
      seat: "orchestrator",
      attention_reason: "disconnected",
    })
    stubLists({ goals: [], tasks: [task], sessions: [orchestrator] })
    const { result } = renderAttention(useAttention)

    await waitFor(() => expect(result.current.items).toHaveLength(2))
    expect(result.current.items).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: task.id,
          task: expect.objectContaining({ title: task.title }),
          goalId: task.goal_id,
          goal: undefined,
        }),
        expect.objectContaining({
          id: orchestrator.id,
          taskId: null,
          goalId: orchestrator.goal_id,
          goal: undefined,
        }),
      ]),
    )
  })

  it("names the task a session ran for and leaves an unknown task unnamed", async () => {
    const task = aTask({ id: "t1", title: "Wire the strip" })
    stubLists({
      tasks: [task],
      sessions: [
        aSession({ id: "s1", task_id: task.id, attention_reason: "disconnected" }),
        aSession({ id: "s2", task_id: null, attention_reason: "disconnected" }),
        aSession({ id: "s3", task_id: "gone", attention_reason: "disconnected" }),
      ],
    })
    const { result } = renderAttention(useAttention)

    await waitFor(() => expect(result.current.items).toHaveLength(3))
    const byId = new Map(result.current.items.map((item) => [item.id, item]))
    expect(byId.get(task.id)?.task?.title).toBe(task.title)
    expect(byId.get("s2")?.taskId).toBeNull()
    expect(byId.get("gone")?.taskId).toBe("gone")
    expect(byId.get("gone")?.task).toBeUndefined()
  })

  it("names a known goal and keeps a row whose goal did not load", async () => {
    const goal = aGoal({ id: "g1", title: "Known" })
    const known = aTask({ id: "t1", goal_id: goal.id, status: "failed" })
    const missing = aTask({ id: "t2", goal_id: "g9", status: "failed" })
    stubLists({ goals: [goal], tasks: [known, missing] })
    const { result } = renderAttention(useAttention)

    await waitFor(() => expect(result.current.items).toHaveLength(2))
    const byId = new Map(result.current.items.map((item) => [item.id, item]))
    expect(byId.get(known.id)?.goal?.title).toBe(goal.title)
    expect(byId.get(missing.id)?.goalId).toBe(missing.goal_id)
    expect(byId.get(missing.id)?.goal).toBeUndefined()
  })

  it("ages an unstamped session from its start and a prompt from its attention time", async () => {
    const started = aSession({
      id: "started",
      task_id: null,
      attention_reason: "disconnected",
      created_at: "2026-01-01T00:00:00Z",
      ended_at: null,
    })
    const prompted = aSession({
      id: "prompted",
      task_id: null,
      attention_reason: "waiting_input",
      created_at: "2026-01-01T00:00:00Z",
      attention_since: "2026-01-03T00:00:00Z",
    })
    stubLists({ sessions: [started, prompted] })
    const { result } = renderAttention(useAttention)

    await waitFor(() => expect(result.current.items).toHaveLength(2))
    expect(result.current.items.find((item) => item.id === started.id)?.at).toBe(started.created_at)
    expect(result.current.items.find((item) => item.id === prompted.id)?.at).toBe(
      prompted.attention_since,
    )
  })

  it("indexes the newest blocked session by task through the board hook", async () => {
    stubLists({
      sessions: [
        aSession({
          id: "old",
          task_id: "t1",
          attention_reason: "disconnected",
          attention_since: "2026-01-01T00:00:00Z",
        }),
        aSession({
          id: "new",
          task_id: "t1",
          attention_reason: "waiting_input",
          attention_since: "2026-01-02T00:00:00Z",
        }),
      ],
    })

    const { result } = renderAttention(useBoardAttention)

    await waitFor(() => expect(result.current.byTask.get("t1")).toBe("waiting_input"))
  })

  it("indexes an orchestrator by goal and starts empty while its list loads", async () => {
    stubLists({ pending: true })
    const pending = renderAttention(useBoardAttention)
    expect(pending.result.current.byTask.size).toBe(0)
    expect(pending.result.current.byGoal.size).toBe(0)

    stubLists({
      sessions: [
        aSession({
          id: "s1",
          goal_id: "g1",
          task_id: null,
          seat: "orchestrator",
          attention_reason: "waiting_input",
        }),
      ],
    })
    const { result } = renderAttention(useBoardAttention)
    await waitFor(() => expect(result.current.byGoal.get("g1")).toBe("waiting_input"))
  })
})
