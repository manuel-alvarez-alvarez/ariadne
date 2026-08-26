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
 *
 * The pins are checked through the mounted form rather than only through
 * `task-form-values.test.ts`, because what they mean is a property of the
 * dialog: one agent select and one model box per slot, each box gated by the
 * select beside it and scoped to what it names, which pick lands in which slot
 * when there are several of them, and — on an edit — what an untouched pair is
 * measured against when the profiles that decide how it was seeded only land
 * after the user has started typing.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { GoalDto, ModelDto, ProfileDto } from "@/api"
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

/** The catalog both model boxes offer, whole. */
const CATALOG: ModelDto[] = [
  { id: "claude-opus-5", agent_kind: "claude_code", description: "Opus tier: deep analysis" },
  { id: "gpt-5.3-codex", agent_kind: "codex", description: "Frontier reasoning: agentic loops" },
]

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
    if (url.pathname === "/v1/models") return answer(CATALOG)
    if (url.pathname === "/v1/tasks") return answer([])
    if (url.pathname === `/v1/goals/${GOAL.id}/tasks`) {
      return answer({ ...CREATED, ...(await request.clone().json()) })
    }
    if (request.method === "PATCH" && url.pathname.startsWith("/v1/tasks/")) return answer(CREATED)
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

/** Picks `model` out of the catalog hanging under the named box. */
async function pickModel(
  user: ReturnType<typeof userEvent.setup>,
  field: string,
  model: string,
): Promise<void> {
  await user.click(await screen.findByRole("combobox", { name: field }))
  await user.click(within(await screen.findByRole("listbox", { name: "Models" })).getByText(model))
}

/** Picks an agent CLI — or "Profile's own" — in the named select. */
async function pickAgent(
  user: ReturnType<typeof userEvent.setup>,
  field: string,
  agent: string,
): Promise<void> {
  await user.click(await screen.findByLabelText(field))
  await user.click(await screen.findByRole("option", { name: agent }))
}

/** The named model box, which the agent select beside it gates. */
async function modelBox(field: string): Promise<HTMLInputElement> {
  return (await screen.findByRole("combobox", { name: field })) as HTMLInputElement
}

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

  /** An engineer profile that runs on a model, for the pin to agree with. */
  const PINNED_ENGINEER: ProfileDto = { ...ENGINEER, model: "claude-opus-5" }

  /** The same task, pinned to exactly what that profile runs on. */
  const PINNED_TASK = {
    ...TASK,
    agent_kind: "claude_code",
    model: PINNED_ENGINEER.model,
  }

  /**
   * The same task on another CLI than its engineer profile's, and on that
   * CLI's own default model rather than a named one.
   */
  const CODEX_TASK = { ...TASK, agent_kind: "codex", model: null }

  function renderEdit(task: unknown = TASK) {
    return renderScreen(<EditTaskDialog task={task as never} open onOpenChange={vi.fn()} />)
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
    expect(posted[0]).toMatchObject({ reviewers: [{ profile: STRICT_REVIEWER.id }] })
  })

  /**
   * The one sequence where "what the box was seeded with" and "what it would
   * be seeded with now" disagree: the dialog opens before the profiles have
   * loaded, so the box shows the task's pin; the user types in another field,
   * which stops the form being re-seeded; the profiles then land and say the
   * pin is only the profile's own model, which would have opened the box
   * empty. The box on screen is still the pin and nobody touched it, so the
   * update has to say nothing about the model — sending it would pin the task
   * to a model the user never chose, and freeze it against a later edit of the
   * profile.
   */
  it("says nothing about a model box nobody touched, though the profiles landed late", async () => {
    const user = userEvent.setup()
    let landProfiles = () => {}
    const profiles = new Promise<void>((resolve) => {
      landProfiles = resolve
    })
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      if (request.method !== "GET") {
        writes.push(`${request.method} ${url.pathname}`)
        posted.push(await request.clone().json())
        return jsonResponse(PINNED_TASK)
      }
      if (url.pathname === "/v1/profiles") {
        await profiles
        return jsonResponse(
          url.searchParams.get("role") === "reviewer" ? [REVIEWER] : [PINNED_ENGINEER],
        )
      }
      if (url.pathname === "/v1/models") return jsonResponse(CATALOG)
      if (url.pathname === "/v1/tasks") return jsonResponse([])
      return new Response("not stubbed", { status: 404 })
    })
    renderScreen(<EditTaskDialog task={PINNED_TASK as never} open onOpenChange={vi.fn()} />)

    // Nothing to read the pin against yet, so the pin is what the box holds.
    const box = (await screen.findByRole("combobox", {
      name: "Engineer model",
    })) as HTMLInputElement
    expect(box.value).toBe(PINNED_ENGINEER.model)

    await user.type(screen.getByLabelText("Title"), "!")
    landProfiles()

    // A reviewer row shows its profile's name only once the profiles are in,
    // so it is the signal — and the box is still what the user saw.
    expect(await screen.findByText("Reviewer")).toBeDefined()
    expect(box.value).toBe(PINNED_ENGINEER.model)
    expect((await screen.findByLabelText("Engineer agent")).textContent).toContain("Claude Code")

    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).not.toHaveProperty("agent_kind")
    expect(posted[0]).not.toHaveProperty("model")
  })

  it("opens on the pinned agent with an empty box, which is that CLI's default", async () => {
    renderEdit(CODEX_TASK)

    // The engineer profile runs on claude_code, so the pin is an override and
    // shows as itself; its model is the CLI's own, so the box stays empty.
    expect((await screen.findByLabelText("Engineer agent")).textContent).toContain("Codex")
    expect((await modelBox("Engineer model")).value).toBe("")
  })

  it("sends the daemon's sentinel when a pin is put back on the profile's own", async () => {
    const user = userEvent.setup()
    renderEdit(CODEX_TASK)

    expect((await screen.findByLabelText("Engineer agent")).textContent).toContain("Codex")
    await pickAgent(user, "Engineer agent", "Profile's own")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).toMatchObject({ agent_kind: "default" })
    expect(posted[0]).not.toHaveProperty("model")
  })

  it("says nothing about a pin left alone, whatever else was edited", async () => {
    const user = userEvent.setup()
    renderEdit(CODEX_TASK)

    expect((await screen.findByLabelText("Engineer agent")).textContent).toContain("Codex")
    await user.type(screen.getByLabelText("Title"), "!")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).not.toHaveProperty("agent_kind")
    expect(posted[0]).not.toHaveProperty("model")
  })
})

/**
 * One pin per slot, chosen on the form that assigns them: an agent CLI, and a
 * model narrowing it to one of that CLI's own.
 */
describe("what the task's agents run on", () => {
  it("keeps each model box shut until its own agent is picked", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    expect((await modelBox("Engineer model")).disabled).toBe(true)
    expect((await modelBox("Reviewer 1 model")).disabled).toBe(true)

    await pickAgent(user, "Engineer agent", "Codex")

    expect((await modelBox("Engineer model")).disabled).toBe(false)
    // One slot at a time: the reviewer's box is its own row's business.
    expect((await modelBox("Reviewer 1 model")).disabled).toBe(true)
  })

  it("offers a slot the models of the agent picked beside it", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await pickAgent(user, "Reviewer 1 agent", "Codex")
    await user.click(await modelBox("Reviewer 1 model"))

    const models = await screen.findByRole("listbox", { name: "Models" })
    expect(within(models).getByText("gpt-5.3-codex")).toBeDefined()
    expect(within(models).queryByText("claude-opus-5")).toBeNull()
  })

  it("sends the engineer's agent and model, and each reviewer's", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    await pickAgent(user, "Engineer agent", "Codex")
    await pickModel(user, "Engineer model", "gpt-5.3-codex")
    await pickAgent(user, "Reviewer 1 agent", "Claude Code")
    await pickModel(user, "Reviewer 1 model", "claude-opus-5")

    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({
      agent_kind: "codex",
      model: "gpt-5.3-codex",
      reviewers: [{ profile: REVIEWER.id, agent_kind: "claude_code", model: "claude-opus-5" }],
    })
  })

  it("sends an agent with an empty box alone, which is that CLI's default", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    await pickAgent(user, "Engineer agent", "Codex")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({ agent_kind: "codex" })
    expect(posted[0]).not.toHaveProperty("model")
  })

  it("leaves an untouched slot out, which runs it on its profile's own", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).not.toHaveProperty("agent_kind")
    expect(posted[0]).not.toHaveProperty("model")
    expect(posted[0]).toMatchObject({ reviewers: [{ profile: REVIEWER.id }] })
  })
})
