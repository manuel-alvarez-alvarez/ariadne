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
 * The reviewers are checked in both modes: the daemon requires one on create,
 * so what the picker shows has to be what is sent whether the user touched it
 * or not, and it is reassignable while the task waits, so the edit form offers
 * it beside the reviewers.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { GoalDto, ProfileDto } from "@/api"
import { aGoal, aProfile } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { CreateTaskDialog, EditTaskDialog } from "./task-form-dialog"

const STAMP = "2026-01-01T00:00:00Z"

const GOAL: GoalDto = aGoal()

const ENGINEER: ProfileDto = aProfile({
  id: "01JPROF000000000000000ENG",
})

const REVIEWER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000REV",
  name: "Reviewer",
  role: "reviewer",
}

/** A second one, for the edit that replaces the task's reviewer list. */
const STRICT_REVIEWER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF00000000000000REV2",
  name: "Strict Reviewer",
  role: "reviewer",
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
  branch: "wire-the-strip-000001",
  depends_on: [],
  engineer_profile_id: ENGINEER.id,
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

    const answer = (payload: unknown) => jsonResponse(payload)

    if (url.pathname === "/v1/profiles") {
      switch (url.searchParams.get("role")) {
        case "reviewer":
          return answer([REVIEWER])
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
  return renderScreen(<CreateTaskDialog goal={GOAL} open onOpenChange={onOpenChange} />)
}

beforeEach(() => {
  writes = []
  posted = []
  stubDaemon()
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

describe("editing a task that has not started", () => {
  const TASK = {
    ...CREATED,
    reviewers: [{ profile_id: REVIEWER.id, agent_kind: null, model: null }],
  }

  function renderEdit() {
    return renderScreen(<EditTaskDialog task={TASK as never} open onOpenChange={vi.fn()} />)
  }

  it("patches the reviewers the user replaced", async () => {
    const user = userEvent.setup()
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      const answer = (payload: unknown) => jsonResponse(payload)
      if (request.method !== "GET") {
        writes.push(`${request.method} ${url.pathname}`)
        posted.push(await request.clone().json())
        return answer(TASK)
      }
      if (url.pathname === "/v1/profiles") return answer([REVIEWER, STRICT_REVIEWER])
      if (url.pathname === "/v1/tasks") return answer([])
      return new Response("not stubbed", { status: 404 })
    })
    renderEdit()

    // The task's own reviewer is what the row starts on, not a default.
    expect(await screen.findByText("Reviewer")).toBeDefined()

    await user.click(await screen.findByLabelText("Reviewer 1"))
    await user.click(await screen.findByRole("option", { name: "Strict Reviewer" }))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).toMatchObject({ reviewer_profiles: [STRICT_REVIEWER.id] })
  })
})
