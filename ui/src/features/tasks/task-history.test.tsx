// @vitest-environment jsdom

/**
 * The transition log, and the one thing it has to get right: each row says the
 * status the daemon actually recorded.
 *
 * The board folds `ready` into Pending and `changes_requested` into In
 * progress so both have a column to be drawn in. A log has no columns, and
 * composed here the fold read as a typo — "Pending → Pending · Ready", "In
 * progress · Changes requested → In progress" — for two transitions that are
 * the most informative ones a task makes.
 *
 * Seeded into the query cache; what the daemon returns is `queries.ts`'s story.
 */

import { screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { qk, type TaskTransitionDto } from "@/api"
import { renderScreen } from "@/test/harness"
import { TaskHistory } from "./task-history"

const TASK_ID = "01JTASK0000000000000000001"
const STAMP = "2026-01-01T00:00:00Z"

function aTransition(overrides: Partial<TaskTransitionDto> = {}): TaskTransitionDto {
  return {
    id: "01JTRAN0000000000000000001",
    actor: "daemon",
    from_status: "pending",
    to_status: "ready",
    reason: null,
    created_at: STAMP,
    ...overrides,
  }
}

function mount(transitions: TaskTransitionDto[]) {
  renderScreen(<TaskHistory taskId={TASK_ID} />, {
    seed: (client) => client.setQueryData(qk.tasks.transitions(TASK_ID), transitions),
  })
}

it("names a folded status as itself, not as the column it is drawn in", () => {
  mount([
    aTransition({ id: "1", from_status: "pending", to_status: "ready" }),
    aTransition({ id: "2", from_status: "under_review", to_status: "changes_requested" }),
    aTransition({ id: "3", from_status: "changes_requested", to_status: "in_progress" }),
  ])

  expect(screen.getAllByText("Ready")).toHaveLength(1)
  expect(screen.getAllByText("Changes requested")).toHaveLength(2)
  expect(screen.queryByText(/·/)).toBeNull()
})

it("falls back to the raw value for a status this build does not know", () => {
  mount([aTransition({ from_status: "in_progress", to_status: "teleported" })])

  expect(screen.getByText("teleported")).toBeDefined()
})

it("says so when the task has not moved at all", () => {
  mount([])

  expect(screen.getByText("The task has not moved yet")).toBeDefined()
})
