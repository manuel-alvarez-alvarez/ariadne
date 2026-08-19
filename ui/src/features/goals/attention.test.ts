import { describe, expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"

import { sessionAttention } from "@/features/sessions/session-display"

import { collectAttention, taskAttentionReason } from "./attention"

function task(overrides: Partial<TaskDto>): TaskDto {
  return {
    branch: "ariadne/task-1",
    created_at: "2026-08-16T10:00:00Z",
    depends_on: [],
    description: "",
    engineer_profile_id: "p1",
    goal_id: "g1",
    id: "t1",
    repo_id: "r1",
    review_round: 0,
    reviewer_profile_ids: [],
    stalled: false,
    status: "in_progress",
    title: "A task",
    updated_at: "2026-08-16T10:00:00Z",
    ...overrides,
  }
}

function session(overrides: Partial<SessionDto>): SessionDto {
  return {
    agent_kind: "claude_code",
    created_at: "2026-08-16T10:00:00Z",
    goal_id: "g1",
    id: "s1",
    profile_id: "p1",
    role: "engineer",
    status: "failed",
    tmux_session: "ariadne-s1",
    ...overrides,
  }
}

function goal(overrides: Partial<GoalDto>): GoalDto {
  return {
    created_at: "2026-08-16T09:00:00Z",
    description: "",
    id: "g1",
    planner_profile_id: "p1",
    repos: [],
    required_approvals: 1,
    status: "active",
    title: "A goal",
    updated_at: "2026-08-16T09:00:00Z",
    ...overrides,
  }
}

describe("taskAttentionReason", () => {
  it("leaves a task that is simply making progress alone", () => {
    expect(taskAttentionReason(task({ status: "in_progress" }))).toBeNull()
    expect(taskAttentionReason(task({ status: "merged" }))).toBeNull()
    expect(taskAttentionReason(task({ status: "under_review" }))).toBeNull()
  })

  it("reports the three statuses that want the user", () => {
    expect(taskAttentionReason(task({ status: "failed" }))).toBe("failed")
    expect(taskAttentionReason(task({ status: "changes_requested" }))).toBe("changes_requested")
    expect(taskAttentionReason(task({ stalled: true }))).toBe("stalled")
  })

  it("prefers the status over the stall flag on top of it", () => {
    expect(taskAttentionReason(task({ status: "failed", stalled: true }))).toBe("failed")
    expect(taskAttentionReason(task({ status: "changes_requested", stalled: true }))).toBe(
      "changes_requested",
    )
  })
})

describe("sessionAttention", () => {
  it("leaves a healthy session alone", () => {
    expect(sessionAttention(session({ status: "running" }))).toBeNull()
    expect(sessionAttention(session({ status: "idle" }))).toBeNull()
    expect(sessionAttention(session({ status: "exited" }))).toBeNull()
  })

  // The same five the daemon can raise, plus the death that raises none.
  it("reports every reason the daemon can flag, and a dead agent", () => {
    expect(
      sessionAttention(session({ status: "running", attention_reason: "waiting_permission" })),
    ).toBe("waiting_permission")
    expect(sessionAttention(session({ status: "idle", attention_reason: "waiting_input" }))).toBe(
      "waiting_input",
    )
    expect(sessionAttention(session({ status: "running", attention_reason: "agent_error" }))).toBe(
      "agent_error",
    )
    expect(sessionAttention(session({ status: "running", attention_reason: "disconnected" }))).toBe(
      "disconnected",
    )
    expect(sessionAttention(session({ status: "idle", attention_reason: "stalled" }))).toBe(
      "stalled",
    )
    expect(sessionAttention(session({ status: "failed" }))).toBe("failed")
  })

  it("prefers the reason over the death that followed it", () => {
    expect(sessionAttention(session({ status: "failed", attention_reason: "agent_error" }))).toBe(
      "agent_error",
    )
  })
})

describe("collectAttention", () => {
  it("keeps only the tasks that want the user", () => {
    const items = collectAttention(
      [goal({})],
      [
        task({ id: "t1", status: "in_progress" }),
        task({ id: "t2", status: "failed" }),
        task({ id: "t3", stalled: true }),
      ],
      [],
    )

    expect(items.map((item) => item.id)).toEqual(["t2", "t3"])
    expect(items.every((item) => item.kind === "task")).toBe(true)
  })

  it("keeps only the sessions that want the user", () => {
    const items = collectAttention(
      [goal({})],
      [],
      [
        session({ id: "s1", status: "running" }),
        session({ id: "s2", status: "failed", ended_at: "2026-08-16T10:00:00Z" }),
        session({
          id: "s3",
          status: "idle",
          attention_reason: "waiting_permission",
          attention_since: "2026-08-16T12:00:00Z",
        }),
      ],
    )

    expect(items.map((item) => item.id)).toEqual(["s3", "s2"])
    expect(items.map((item) => item.kind === "session" && item.reason)).toEqual([
      "waiting_permission",
      "failed",
    ])
  })

  it("mixes tasks and sessions into one list, most recently moved first", () => {
    const items = collectAttention(
      [goal({})],
      [
        task({ id: "t1", status: "failed", updated_at: "2026-08-16T10:00:00Z" }),
        task({ id: "t2", status: "changes_requested", updated_at: "2026-08-16T12:00:00Z" }),
      ],
      [session({ id: "s1", ended_at: "2026-08-16T11:00:00Z" })],
    )

    expect(items.map((item) => item.id)).toEqual(["t2", "s1", "t1"])
  })

  it("ages a session that has no end by when it started", () => {
    const [item] = collectAttention([], [], [session({ created_at: "2026-08-16T08:00:00Z" })])

    expect(item?.at).toBe("2026-08-16T08:00:00Z")
  })

  // The row is about the waiting, not about the agent: a session that has been
  // blocked on a prompt for an hour reads as an hour old, not as however long
  // ago it started.
  it("ages a flagged session by when its reason was raised", () => {
    const [item] = collectAttention(
      [],
      [],
      [
        session({
          status: "running",
          attention_reason: "waiting_permission",
          attention_since: "2026-08-16T11:00:00Z",
          created_at: "2026-08-16T08:00:00Z",
        }),
      ],
    )

    expect(item?.at).toBe("2026-08-16T11:00:00Z")
  })

  // The goal is where the row sits; the task is what the agent was doing.
  it("names the task a session was run for, and nothing for a planner's", () => {
    const items = collectAttention(
      [goal({})],
      [task({ id: "t1", title: "Wire the strip" })],
      [
        session({ id: "s1", task_id: "t1" }),
        session({ id: "s2", role: "planner" }),
        session({ id: "s3", task_id: "gone" }),
      ],
    )

    const tasks = items.map((item) => item.kind === "session" && item.task?.title)
    expect(tasks).toEqual(["Wire the strip", undefined, undefined])
  })

  it("names the goal each row belongs to, and keeps rows whose goal is missing", () => {
    const items = collectAttention(
      [goal({ id: "g1", title: "Known" })],
      [
        task({ id: "t1", goal_id: "g1", status: "failed", updated_at: "2026-08-16T12:00:00Z" }),
        task({ id: "t2", goal_id: "g9", status: "failed", updated_at: "2026-08-16T11:00:00Z" }),
      ],
      [],
    )

    expect(items[0]?.goal?.title).toBe("Known")
    expect(items[1]?.goalId).toBe("g9")
    expect(items[1]?.goal).toBeUndefined()
  })

  // A failed query leaves its list undefined while the others answered: the
  // rows that did load are still a list, not an empty screen.
  it("reads a list that only partly loaded", () => {
    const items = collectAttention(undefined, undefined, [session({ id: "s1" })])

    expect(items.map((item) => item.id)).toEqual(["s1"])
    expect(items[0]?.goal).toBeUndefined()
  })
})
