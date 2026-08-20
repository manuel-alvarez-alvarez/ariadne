import { describe, expect, it } from "vitest"

import type { TaskDto, TaskStatus } from "@/api"
import {
  BOARD_STATUSES,
  canCancel,
  canEdit,
  compareByAttention,
  displayLabel,
  primaryStatus,
  subStatus,
} from "./status"

function task(id: string, status: TaskStatus, extra: Partial<TaskDto> = {}): TaskDto {
  return {
    id,
    goal_id: "g1",
    title: id,
    description: "",
    status,
    branch: `ariadne/${id}`,
    repo_id: "r1",
    depends_on: [],
    engineer_profile_id: "p1",
    reviewers: [],
    review_round: 0,
    stalled: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...extra,
  }
}

describe("compareByAttention", () => {
  it("puts what is waiting on the user before what the agents are still doing", () => {
    const ordered = [
      task("merged", "merged"),
      task("in-progress", "in_progress"),
      task("approved", "approved"),
      task("failed", "failed"),
      task("pending", "pending"),
    ].sort(compareByAttention)

    expect(ordered.map((t) => t.id)).toEqual([
      "failed",
      "approved",
      "in-progress",
      "pending",
      "merged",
    ])
  })

  it("lifts a stalled task above everything, whatever it is parked in", () => {
    const ordered = [task("failed", "failed"), task("stalled", "pending", { stalled: true })].sort(
      compareByAttention,
    )

    expect(ordered.map((t) => t.id)).toEqual(["stalled", "failed"])
  })

  it("orders the same status by what moved last", () => {
    const ordered = [
      task("old", "ready", { updated_at: "2026-01-01T00:00:00Z" }),
      task("new", "ready", { updated_at: "2026-02-01T00:00:00Z" }),
    ].sort(compareByAttention)

    expect(ordered.map((t) => t.id)).toEqual(["new", "old"])
  })
})

describe("canEdit", () => {
  it("allows editing only before an engineer has started", () => {
    const editable: TaskStatus[] = ["pending", "ready"]
    const frozen: TaskStatus[] = [
      "in_progress",
      "under_review",
      "changes_requested",
      "approved",
      "merging",
      "merged",
      "cancelled",
      "failed",
    ]

    for (const status of editable) expect(canEdit(status)).toBe(true)
    for (const status of frozen) expect(canEdit(status)).toBe(false)
  })
})

describe("the ready fold", () => {
  it("gives the board four columns, ready not among them", () => {
    expect(BOARD_STATUSES).toEqual(["pending", "in_progress", "under_review", "merged"])
  })

  it("puts a ready task in the pending column, still saying it is ready", () => {
    expect(primaryStatus("ready")).toBe("pending")
    expect(subStatus("ready")?.label).toBe("Ready")
    expect(displayLabel("ready")).toBe("Pending · Ready")
  })

  it("leaves what the user may do to a ready task alone", () => {
    // The fold is where the card is drawn, nothing else: a ready task is still
    // pre-start, so it is still editable and cancellable.
    expect(canEdit("ready")).toBe(true)
    expect(canCancel("ready")).toBe(true)
  })

  it("keeps ready ahead of pending, so a stuck task sorts above a blocked one", () => {
    const ordered = [
      task("dependency-blocked", "pending"),
      task("waiting-for-an-engineer", "ready"),
    ].sort(compareByAttention)

    expect(ordered.map((t) => t.id)).toEqual(["waiting-for-an-engineer", "dependency-blocked"])
  })
})
