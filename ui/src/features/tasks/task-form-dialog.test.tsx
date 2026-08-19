// @vitest-environment jsdom

/**
 * The task form's dismissal, which is the one thing about it that cannot be
 * left to the daemon.
 *
 * The brief is what a whole task is built from, so an outside press a few
 * paragraphs in has to ask — and the two profiles the form preselects on open
 * are its own doing rather than the user's, so a glance at the dialog must
 * still close it with nothing asked.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { GoalDto, ProfileDto } from "@/api"

import { CreateTaskDialog } from "./task-form-dialog"

/** Hoisted for the reason the other dialog tests give: the client takes its
 * `fetch` when `@/api` is imported, so a later stub is one it never sees. */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const STAMP = "2026-01-01T00:00:00Z"

const GOAL: GoalDto = {
  id: "01JGOAL0000000000000000001",
  title: "Ship the board",
  description: "",
  planner_profile_id: "01JPROF00000000000000PLAN",
  repos: [],
  required_approvals: 1,
  status: "active",
  created_at: STAMP,
  updated_at: STAMP,
}

const ENGINEER: ProfileDto = {
  id: "01JPROF000000000000000ENG",
  name: "Engineer",
  role: "engineer",
  agent_kind: "claude_code",
  model: null,
  system_prompt: "",
  created_at: STAMP,
  updated_at: STAMP,
}

const REVIEWER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000REV",
  name: "Reviewer",
  role: "reviewer",
}

let writes: string[] = []

/** The reads the dialog does; a write would be a failure, so it is recorded. */
function stubDaemon() {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    if (request.method !== "GET") writes.push(`${request.method} ${url.pathname}`)

    const answer = (payload: unknown) =>
      new Response(JSON.stringify(payload), {
        status: 200,
        headers: { "content-type": "application/json" },
      })

    if (url.pathname === "/v1/profiles") {
      const role = url.searchParams.get("role")
      return answer(role === "reviewer" ? [REVIEWER] : [ENGINEER])
    }
    if (url.pathname === "/v1/tasks") return answer([])
    return new Response("not stubbed", { status: 404 })
  })
}

function renderDialog(onOpenChange: (open: boolean) => void) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  return render(
    <QueryClientProvider client={queryClient}>
      <CreateTaskDialog goal={GOAL} open onOpenChange={onOpenChange} />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  writes = []
  daemonFetch.mockReset()
  stubDaemon()
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe("dismissing the dialog", () => {
  it("closes an untouched form straight away, preselected profiles and all", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(onOpenChange)

    // The preselects are what this is about: wait until they have happened.
    expect(await screen.findByText("Engineer")).toBeDefined()
    expect(await screen.findByText("Reviewer")).toBeDefined()

    await user.click(screen.getByRole("button", { name: "Cancel" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })

  it("asks before dropping a typed brief, and keeps it when the answer is no", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(onOpenChange)

    await user.type(screen.getByLabelText("Description"), "Rewrite the scheduler.")
    await user.keyboard("{Escape}")

    expect(await screen.findByText("Discard changes?")).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Keep editing" }))

    expect((screen.getByLabelText("Description") as HTMLTextAreaElement).value).toBe(
      "Rewrite the scheduler.",
    )
    expect(onOpenChange).not.toHaveBeenCalled()
  })

  it("closes and drops the draft once the discard is confirmed", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(onOpenChange)

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    await user.click(await screen.findByRole("button", { name: "Discard" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(writes).toEqual([])
  })
})
