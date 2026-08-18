import { describe, expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"

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

  it("mixes tasks and failed sessions into one list, most recently moved first", () => {
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
