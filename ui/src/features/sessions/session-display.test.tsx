// @vitest-environment jsdom

/**
 * What a session's status looks like, which is the half of the ramp it takes.
 *
 * The session statuses and the task statuses are drawn from one ramp
 * (`index.css`), so a step has to mean the same thing whichever of the two is
 * wearing it. Running used to take the green a task takes when it is *merged*
 * and idle the accent that means work is happening, which said the opposite of
 * what was going on: the brightest thing on the board was an agent that had
 * finished nothing, and the session waiting on a person looked busy.
 */

import { render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import type { SessionStatus } from "@/api"
import { TASK_STATUS_META } from "@/features/tasks/status"
import { isLiveStatus, SESSION_STATUS_META, SessionStatusBadge } from "./session-display"

/** The dot's classes, off the badge as it is actually rendered. */
function dotClasses(status: SessionStatus): string {
  render(<SessionStatusBadge status={status} />)
  const dot = screen.getByText(SESSION_STATUS_META[status].label).firstElementChild
  if (!(dot instanceof HTMLElement)) throw new Error(`no dot on the ${status} badge`)
  return dot.className
}

describe("the session status colours", () => {
  it("draws a session on the step that means what the session is doing", () => {
    // Work happening is the accent, whatever is doing it; waiting on a person
    // is the warn step every attention reason already sits on; not started yet
    // is the pending grey.
    expect(SESSION_STATUS_META.running.dot).toBe("bg-status-active")
    expect(SESSION_STATUS_META.idle.dot).toBe("bg-status-warn")
    expect(SESSION_STATUS_META.starting.dot).toBe("bg-status-pending")
  })

  it("leaves the colours that carry a task's meaning to the tasks", () => {
    // Merged green and review violet say something about a *task*, and a
    // session wearing one of them would be saying it about the wrong thing.
    const taken = [TASK_STATUS_META.merged.dot, TASK_STATUS_META.under_review.dot]
    const live: SessionStatus[] = ["starting", "running", "idle"]

    expect(live.map((status) => SESSION_STATUS_META[status].dot)).not.toContain(taken[0])
    expect(live.map((status) => SESSION_STATUS_META[status].dot)).not.toContain(taken[1])
  })

  it("puts that colour on the badge it renders", () => {
    expect(dotClasses("idle")).toContain("bg-status-warn")
    expect(dotClasses("running")).toContain("bg-status-active")
  })

  it("keeps the pulse to the sessions with a pane that may still speak", () => {
    expect(isLiveStatus("idle")).toBe(true)
    expect(isLiveStatus("exited")).toBe(false)
    expect(isLiveStatus("failed")).toBe(false)
  })
})
