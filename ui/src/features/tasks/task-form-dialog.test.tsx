// @vitest-environment jsdom

/**
 * The task form's dismissal, which is the one thing about it that cannot be
 * left to the daemon, and the assignments it puts on the wire.
 *
 * The brief is what a whole task is built from, so an outside press a few
 * paragraphs in has to ask — and the three profiles the form preselects on
 * open are its own doing rather than the user's, so a glance at the dialog
 * must still close it with nothing asked.
 *
 * The integrator is checked in both modes: the daemon requires one on create,
 * so what the picker shows has to be what is sent whether the user touched it
 * or not, and it is reassignable while the task waits, so the edit form offers
 * it beside the reviewers.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { GoalDto, ProfileDto } from "@/api"

import { CreateTaskDialog, EditTaskDialog } from "./task-form-dialog"

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

/** The built-in Integrator: the one the form preselects, by its id. */
const INTEGRATOR: ProfileDto = {
  ...ENGINEER,
  id: "00000000000000000000000004",
  name: "Integrator",
  role: "integrator",
}

/** One the user made, sorting ahead of the built-in so preselecting the
 * built-in cannot be an accident. */
const CUSTOM_INTEGRATOR: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000INT",
  name: "Fleet Lander",
  role: "integrator",
}

/** The bodies of the writes the dialog made, in order. */
let posted: unknown[] = []

/** Enough of a task for the mutation's cache write and its toast. */
const CREATED = {
  id: "01JTASK0000000000000000001",
  goal_id: GOAL.id,
  repo_id: "01JREPO0000000000000000001",
  title: "Wire the strip",
  description: "",
  status: "pending",
  branch: "ariadne/task-01JTASK0000000000000000001",
  depends_on: [],
  engineer_profile_id: ENGINEER.id,
  integrator_profile_id: INTEGRATOR.id,
  reviewers: [],
  review_round: 0,
  stalled: false,
  created_at: STAMP,
  updated_at: STAMP,
}

let writes: string[] = []

/** The reads the dialog does; a write would be a failure, so it is recorded. */
function stubDaemon() {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    if (request.method !== "GET") {
      writes.push(`${request.method} ${url.pathname}`)
      posted.push(await request.clone().json())
    }

    const answer = (payload: unknown) =>
      new Response(JSON.stringify(payload), {
        status: 200,
        headers: { "content-type": "application/json" },
      })

    if (url.pathname === "/v1/profiles") {
      switch (url.searchParams.get("role")) {
        case "reviewer":
          return answer([REVIEWER])
        case "integrator":
          return answer([CUSTOM_INTEGRATOR, INTEGRATOR])
        default:
          return answer([ENGINEER])
      }
    }
    if (url.pathname === "/v1/tasks") return answer([])
    if (url.pathname === `/v1/goals/${GOAL.id}/tasks`) {
      return answer({ ...CREATED, ...(await request.clone().json()) })
    }
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
  posted = []
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
    expect(await screen.findByText("Integrator")).toBeDefined()
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

describe("the integrator assignment", () => {
  it("preselects the built-in integrator and sends it untouched", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    // Not simply the first of the list — "Fleet Lander" sorts ahead of it.
    expect(await screen.findByText("Integrator")).toBeDefined()

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({ integrator_profile: INTEGRATOR.id })
  })

  it("sends the integrator the user picked instead", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    await user.click(await screen.findByLabelText("Integrator profile"))
    await user.click(await screen.findByRole("option", { name: "Fleet Lander" }))
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({ integrator_profile: CUSTOM_INTEGRATOR.id })
  })
})

describe("editing a task that has not started", () => {
  const TASK = {
    ...CREATED,
    reviewers: [{ profile_id: REVIEWER.id, agent_kind: null, model: null }],
  }

  function renderEdit() {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    })
    return render(
      <QueryClientProvider client={queryClient}>
        <EditTaskDialog task={TASK as never} open onOpenChange={vi.fn()} />
      </QueryClientProvider>,
    )
  }

  it("offers the integrator beside the reviewers and patches the new one", async () => {
    const user = userEvent.setup()
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      const answer = (payload: unknown) =>
        new Response(JSON.stringify(payload), {
          status: 200,
          headers: { "content-type": "application/json" },
        })
      if (request.method !== "GET") {
        writes.push(`${request.method} ${url.pathname}`)
        posted.push(await request.clone().json())
        return answer({ ...TASK, integrator_profile_id: CUSTOM_INTEGRATOR.id })
      }
      if (url.pathname === "/v1/profiles") {
        return answer(
          url.searchParams.get("role") === "integrator"
            ? [CUSTOM_INTEGRATOR, INTEGRATOR]
            : [REVIEWER],
        )
      }
      if (url.pathname === "/v1/tasks") return answer([])
      return new Response("not stubbed", { status: 404 })
    })
    renderEdit()

    // The task's own integrator is what the picker starts on, not a default.
    expect(await screen.findByText("Integrator")).toBeDefined()

    await user.click(await screen.findByLabelText("Integrator profile"))
    await user.click(await screen.findByRole("option", { name: "Fleet Lander" }))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).toMatchObject({ integrator_profile: CUSTOM_INTEGRATOR.id })
  })
})
