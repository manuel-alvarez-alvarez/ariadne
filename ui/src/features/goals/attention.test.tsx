// @vitest-environment jsdom

/**
 * `useAttention`'s own rule for `exhausted`, for a task a recovery item
 * already names, and for a recovery item with no goal or session of its
 * own: `GET /v1/attention`, not the bare session flag or task status,
 * decides whether any of them raises a row.
 *
 * `AttentionAlerts`' title count is the simplest surface this list answers
 * through — the rest of its folding (a task and its session as one row, the
 * ordering, the three stamps a row ages by) is `attention-alerts.test.tsx`'s
 * and the CLI's own `attention.rs` tests' business, not this task's.
 */

import type { QueryClient } from "@tanstack/react-query"
import { waitFor } from "@testing-library/react"
import { beforeEach, expect, it, vi } from "vitest"

import type { AttentionItemDto, SessionDto } from "@/api"
import { sessionTerminalFrom } from "@/routes/paths"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { type AttentionItem, attentionAffectedLinks } from "./attention"
import { AttentionAlerts } from "./attention-alerts"

const GOAL = aGoal()
/** Healthy by default; the duplicate-row test fails it on its own. */
const TASK = aTask({ goal_id: GOAL.id })

/** What the daemon answers right now; a test sets these before it renders. */
let sessions: SessionDto[]
let recoveryItems: AttentionItemDto[]
let recoveryComplete: boolean

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
            ? { items: recoveryItems, complete: recoveryComplete }
            : aSessionPage(sessions)
    return Promise.resolve(jsonResponse(body))
  })
}

beforeEach(() => {
  sessions = []
  recoveryItems = []
  recoveryComplete = true
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

/** Every query this screen reads has answered — a redraw is settled, not a
 * title that still happens to read "Ariadne Desktop" because nothing has
 * come back yet. */
async function settled(queryClient: QueryClient) {
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
}

it("raises no row for an exhausted session recovery has not given up on", async () => {
  sessions = [aSession({ attention_reason: "exhausted", attention_since: "2026-01-01" })]
  const { queryClient } = renderAlerts()
  await settled(queryClient)

  expect(document.title).toBe("Ariadne Desktop")
})

it("raises a row for an exhausted session only once a quota item names it", async () => {
  const exhausted = aSession({ attention_reason: "exhausted", attention_since: "2026-01-01" })
  sessions = [exhausted]
  recoveryItems = [
    {
      id: "recovery:quota:claude-sonnet-5",
      producer: "recovery",
      reason: "quota",
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
      producer: "recovery",
      reason: "resource",
      summary: "The daemon ran out of file descriptors.",
      required_action: "Free file descriptors, then retry the task.",
      since: "2026-01-01T00:00:00Z",
      affected: [{ kind: "task", id: "01OTHER", label: "another task" }],
      target: { kind: "task", task_id: "01OTHER" },
    },
  ]
  renderAlerts()

  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
})

it("does not also count a task's own row once a recovery item already names it", async () => {
  const failed = aTask({ id: "01FAILED", goal_id: GOAL.id, status: "failed" })
  recoveryItems = [
    {
      id: "recovery:resource:descriptor-limit",
      producer: "recovery",
      reason: "resource",
      summary: "The daemon ran out of file descriptors.",
      required_action: "Free file descriptors, then retry the task.",
      since: "2026-01-01T00:00:00Z",
      affected: [{ kind: "task", id: failed.id, label: failed.title }],
      target: { kind: "task", task_id: failed.id },
    },
  ]
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const { pathname } = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body =
      pathname === "/v1/goals"
        ? [GOAL]
        : pathname === "/v1/tasks"
          ? [failed]
          : pathname === "/v1/attention"
            ? { items: recoveryItems, complete: true }
            : aSessionPage([])
    return Promise.resolve(jsonResponse(body))
  })
  const { queryClient } = renderAlerts()
  await settled(queryClient)

  // One row — the recovery item's — not two: `failed`'s own generic row
  // would otherwise also count this same task.
  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
})

it("raises no row for a failed task once a complete recovery read has nothing to say about it", async () => {
  // A complete, empty `/v1/attention` is what the daemon answers while a
  // failed task's own orchestrator is still the one being told of it —
  // that silence is the whole point of the suppression (009), and it must
  // read the same as "recovery looked and found nothing", not as "recovery
  // never named this task so fall back to the bare status". A membership
  // check on recovery's own `affected` lists cannot tell those two apart;
  // only `complete` can.
  const failed = aTask({ id: "01FAILED", goal_id: GOAL.id, status: "failed" })
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const { pathname } = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body =
      pathname === "/v1/goals"
        ? [GOAL]
        : pathname === "/v1/tasks"
          ? [failed]
          : pathname === "/v1/attention"
            ? { items: [], complete: true }
            : aSessionPage([])
    return Promise.resolve(jsonResponse(body))
  })
  const { queryClient } = renderAlerts()
  await settled(queryClient)

  expect(document.title).toBe("Ariadne Desktop")
})

it("raises no row for a task-tied stalled or disconnected session", async () => {
  sessions = [
    aSession({
      id: "01STALLED",
      task_id: TASK.id,
      attention_reason: "stalled",
      attention_since: "2026-01-01",
    }),
    aSession({
      id: "01DEAD",
      task_id: TASK.id,
      attention_reason: "disconnected",
    }),
  ]
  const { queryClient } = renderAlerts()
  await settled(queryClient)

  // Automatic recovery (a nudge, then a relaunch) is still trying both —
  // only once the task itself fails is either this list's business.
  expect(document.title).toBe("Ariadne Desktop")
})

it("raises a row for a stalled or disconnected session with no task", async () => {
  sessions = [
    aSession({
      id: "01ORCH",
      task_id: null,
      seat: "orchestrator",
      attention_reason: "disconnected",
    }),
  ]
  const { queryClient } = renderAlerts()
  await settled(queryClient)

  expect(document.title).toBe("(1) Ariadne")
})

it("never reads an incomplete recovery read as nothing needing attention", async () => {
  recoveryComplete = false
  const { queryClient } = renderAlerts()
  await settled(queryClient)

  // Zero items under a read that did not finish is unknown, not quiet —
  // the title says so, distinct from the true all-clear.
  expect(document.title).toBe("(!) Ariadne")
})

function aRecoveryRow(item: AttentionItemDto): AttentionItem {
  return {
    id: item.id,
    goalId: "",
    goal: undefined,
    at: item.since,
    taskId: null,
    task: undefined,
    taskReason: null,
    session: undefined,
    sessionReason: null,
    recovery: item,
  }
}

it("gives a grouped quota item's every session its own link to its own terminal", () => {
  const row = aRecoveryRow({
    id: "recovery:quota:claude-sonnet-5:spent-its-automatic-switch-budget",
    producer: "recovery",
    reason: "quota",
    summary: "claude-sonnet-5 hit a usage limit.",
    required_action: "Raise the quota.",
    since: "2026-01-01T00:00:00Z",
    affected: [
      { kind: "session", id: "01SA", label: "01TASKA (3 switch(es))" },
      { kind: "session", id: "01SB", label: "01TASKB (3 switch(es))" },
    ],
    target: { kind: "console", session_id: "01SA" },
  })
  const search = new URLSearchParams()
  const links = attentionAffectedLinks(row, search, "/goals")
  expect(links?.map((link) => link.id)).toEqual(["01SA", "01SB"])
  const [first, second] = links ?? []
  expect(first).toBeDefined()
  expect(second).toBeDefined()
  // Each session's own link, not a copy of the item's own target (which
  // would open only 01SA for both rows) — asserted against the exact
  // destination `sessionTerminalFrom` itself builds, not only the id
  // copied into the helper's result.
  expect(first?.to).toEqual(sessionTerminalFrom("/goals", search, "01SA"))
  expect(second?.to).toEqual(sessionTerminalFrom("/goals", search, "01SB"))
  expect(first?.to).not.toEqual(second?.to)
})

it("gives a single-session quota item no extra links of its own", () => {
  const row = aRecoveryRow({
    id: "recovery:quota:claude-sonnet-5:spent-its-automatic-switch-budget",
    producer: "recovery",
    reason: "quota",
    summary: "claude-sonnet-5 hit a usage limit.",
    required_action: "Raise the quota.",
    since: "2026-01-01T00:00:00Z",
    affected: [{ kind: "session", id: "01SA", label: "01TASKA (3 switch(es))" }],
    target: { kind: "console", session_id: "01SA" },
  })
  expect(attentionAffectedLinks(row, new URLSearchParams(), "/goals")).toBeNull()
})
