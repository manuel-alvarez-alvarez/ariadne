import { describe, expect, it } from "vitest"

import type { TaskDto } from "@/api"

import { taskAttentionReason } from "./queries"

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
