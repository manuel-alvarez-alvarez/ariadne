// @vitest-environment jsdom

/**
 * `useAttention`'s own rule for `exhausted`, and for a recovery item with no
 * goal or session of its own: `GET /v1/attention`, not the bare session
 * flag, decides whether either raises a row.
 *
 * `AttentionAlerts`' title count is the simplest surface this list answers
 * through — the rest of its folding (a task and its session as one row, the
 * ordering, the three stamps a row ages by) is `attention-alerts.test.tsx`'s
 * and the CLI's own `attention.rs` tests' business, not this task's.
 */

import { waitFor } from "@testing-library/react"
import { beforeEach, expect, it, vi } from "vitest"

import type { AttentionItemDto, SessionDto } from "@/api"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { AttentionAlerts } from "./attention-alerts"

const GOAL = aGoal()
const TASK = aTask({ goal_id: GOAL.id })

/** What the daemon answers right now; a test sets both before it renders. */
let sessions: SessionDto[]
let recoveryItems: AttentionItemDto[]

function stubDaemon() {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const { pathname } = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body =
      pathname === "/v1/goals"
        ? [GOAL]
        : pathname === "/v1/tasks"
          ? [TASK]
          : pathname === "/v1/attention"
            ? { items: recoveryItems, complete: true }
            : aSessionPage(sessions)
    return Promise.resolve(jsonResponse(body))
  })
}

beforeEach(() => {
  sessions = []
  recoveryItems = []
  document.title = "Ariadne Desktop"
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  }))
  stubDaemon()
})

function renderAlerts() {
  return renderScreen(<AttentionAlerts />, { route: "/profiles" })
}

it("raises no row for an exhausted session recovery has not given up on", async () => {
  sessions = [aSession({ attention_reason: "exhausted", attention_since: "2026-01-01" })]
  renderAlerts()

  await waitFor(() => expect(document.title).toBe("Ariadne Desktop"))
})

it("raises a row for an exhausted session only once a quota item names it", async () => {
  const exhausted = aSession({ attention_reason: "exhausted", attention_since: "2026-01-01" })
  sessions = [exhausted]
  recoveryItems = [
    {
      id: "recovery:quota:claude-sonnet-5",
      cause: "quota",
      summary: "claude-sonnet-5 hit a usage limit.",
      required_action: "Raise the quota, or switch models.",
      since: "2026-01-01T00:00:00Z",
      affected: [{ kind: "session", id: exhausted.id, label: "claude-sonnet-5" }],
      target: { kind: "console", session_id: exhausted.id },
    },
  ]
  renderAlerts()

  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
})

it("counts a resource or configuration item that names no goal or session", async () => {
  recoveryItems = [
    {
      id: "recovery:resource:descriptor-limit",
      cause: "resource",
      summary: "The daemon ran out of file descriptors.",
      required_action: "Free file descriptors, then retry the task.",
      since: "2026-01-01T00:00:00Z",
      affected: [{ kind: "task", id: TASK.id, label: TASK.title }],
      target: { kind: "task", task_id: TASK.id },
    },
  ]
  renderAlerts()

  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
})
