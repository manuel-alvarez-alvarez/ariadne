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
      "integrating",
      "merged",
      "cancelled",
      "failed",
    ]

    for (const status of editable) expect(canEdit(status)).toBe(true)
    for (const status of frozen) expect(canEdit(status)).toBe(false)
  })
})

describe("the board columns", () => {
  it("is one column per pipeline stage, in pipeline order", () => {
    expect(BOARD_STATUSES).toEqual([
      "pending",
      "in_progress",
      "under_review",
      "integrating",
      "merged",
    ])
  })

  it("leaves the folded statuses out: they are badges, not columns", () => {
    for (const folded of ["ready", "changes_requested", "approved"] as const) {
      expect(BOARD_STATUSES).not.toContain(folded)
      expect(subStatus(folded)).toBeDefined()
    }
  })
})

describe("the ready fold", () => {
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

describe("the approved fold", () => {
  it("puts an approved task in the Integrating column, still saying it is approved", () => {
    // Forwards, not back: the reviewers are done with it and the integrator is
    // the next thing to touch it.
    expect(primaryStatus("approved")).toBe("integrating")
    expect(subStatus("approved")?.label).toBe("Approved")
    expect(displayLabel("approved")).toBe("Integrating · Approved")
  })

  it("gives integrating a column, and a label of its own", () => {
    expect(primaryStatus("integrating")).toBe("integrating")
    expect(subStatus("integrating")).toBeUndefined()
    expect(displayLabel("integrating")).toBe("Integrating")
  })

  it("ranks an integrating task above everything the agents can finish alone", () => {
    // A published pull request is the one thing on the board whose next step
    // is a person's, so it sorts under nothing but a failure.
    const ordered = [
      task("under-review", "under_review"),
      task("changes-requested", "changes_requested"),
      task("integrating", "integrating"),
      task("failed", "failed"),
      task("approved", "approved"),
    ].sort(compareByAttention)

    expect(ordered.map((t) => t.id)).toEqual([
      "failed",
      "integrating",
      "approved",
      "changes-requested",
      "under-review",
    ])
  })
})
