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
 * dialog: one model box per slot, holding the whole choice — the agent CLI and
 * the model of it — which pick lands in which slot when there are several of
 * them, and — on an edit — what an untouched box is measured against when the
 * profiles that decide how it was seeded only land after the user has started
 * typing.
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

/** The catalog every model box offers, whole. */
const CATALOG: ModelDto[] = [
  {
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
    efforts: [],
  },
  {
    id: "codex:gpt-5.3-codex",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
    efforts: [],
  },
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

/** The named model box: one per slot, and the whole choice for it. */
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
    reviewers: [{ profile_id: REVIEWER.id, model: null }],
  }

  /** An engineer profile that runs on a model, for the pin to agree with. */
  const PINNED_ENGINEER: ProfileDto = { ...ENGINEER, model: "claude_code:claude-opus-5" }

  /** The same task, pinned to exactly what that profile runs on. */
  const PINNED_TASK = { ...TASK, model: PINNED_ENGINEER.model }

  /**
   * The same task on another CLI than its engineer profile's, and on that
   * CLI's own default model rather than a named one.
   */
  const CODEX_TASK = { ...TASK, model: "codex" }

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

    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).not.toHaveProperty("model")
  })

  it("opens on the pinned agent CLI alone, which is that CLI's default model", async () => {
    renderEdit(CODEX_TASK)

    // The engineer profile runs on claude_code, so the pin is an override and
    // shows as itself — the CLI on its own, with no model after it.
    expect((await modelBox("Engineer model")).value).toBe("codex")
  })

  it("sends the daemon's sentinel when a pin is emptied back to the profile's own", async () => {
    const user = userEvent.setup()
    renderEdit(CODEX_TASK)

    await user.clear(await modelBox("Engineer model"))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).toMatchObject({ model: "default" })
  })

  it("says nothing about a pin left alone, whatever else was edited", async () => {
    const user = userEvent.setup()
    renderEdit(CODEX_TASK)

    expect((await modelBox("Engineer model")).value).toBe("codex")
    await user.type(screen.getByLabelText("Title"), "!")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).not.toHaveProperty("model")
  })
})

/**
 * One pin per slot, chosen on the form that assigns them: one id, naming the
 * agent CLI and then the model of it.
 */
describe("what the task's agents run on", () => {
  it("offers every slot the catalog whole, grouped by agent CLI", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.click(await modelBox("Reviewer 1 model"))

    const models = await screen.findByRole("listbox", { name: "Models" })
    expect(within(models).getByText("codex:gpt-5.3-codex")).toBeDefined()
    expect(within(models).getByText("claude_code:claude-opus-5")).toBeDefined()
    expect(within(models).getByText("Codex")).toBeDefined()
  })

  it("sends the engineer's model, and each reviewer's, from its own box", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    await pickModel(user, "Engineer model", "codex:gpt-5.3-codex")
    await pickModel(user, "Reviewer 1 model", "claude_code:claude-opus-5")

    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({
      model: "codex:gpt-5.3-codex",
      reviewers: [{ profile: REVIEWER.id, model: "claude_code:claude-opus-5" }],
    })
  })

  it("sends an agent CLI typed on its own, which is that CLI's default", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    await user.type(await modelBox("Engineer model"), "codex")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({ model: "codex" })
  })

  it("keeps an id whose model half carries colons of its own", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    await user.type(await modelBox("Engineer model"), "opencode:ollama/llama3:8b")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({ model: "opencode:ollama/llama3:8b" })
  })

  it("refuses a model naming no agent CLI, on the row it was typed in", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    await user.type(await modelBox("Reviewer 1 model"), "claude-opus-5")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    expect(await screen.findByText(/claude_code:claude-opus-5/)).toBeDefined()
    expect(writes).toEqual([])
  })

  it("leaves an untouched slot out, which runs it on its profile's own", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).not.toHaveProperty("model")
    expect(posted[0]).toMatchObject({ reviewers: [{ profile: REVIEWER.id }] })
  })
})

/**
 * The brief is what the engineer builds from and it is read as Markdown, so
 * the box writes it and reads it back in place — and the form, whose longest
 * field is that box, can be finished without reaching for the mouse.
 */
describe("writing the task's brief", () => {
  it("keeps a plain Enter in the brief a newline, and the form open", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    const brief = screen.getByLabelText("Description") as HTMLTextAreaElement
    await user.type(brief, "Wire the strip{Enter}then test it")

    expect(brief.value).toBe("Wire the strip\nthen test it")
    expect(writes).toEqual([])
  })

  it("creates the task on the chord, typed from inside the brief", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByText("Engineer")).toBeDefined()
    expect(await screen.findByText("Reviewer")).toBeDefined()

    await user.type(screen.getByLabelText("Description"), "Wire the strip")
    await user.keyboard("{Meta>}{Enter}{/Meta}")

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({ title: "Wire the strip", description: "Wire the strip" })
  })

  it("saves an edited task on Ctrl+Enter too", async () => {
    const user = userEvent.setup()
    const task = { ...CREATED, reviewers: [{ profile_id: REVIEWER.id, model: null }] }
    renderScreen(<EditTaskDialog task={task as never} open onOpenChange={vi.fn()} />)

    await user.type(await screen.findByLabelText("Description"), "One more thing")
    await user.keyboard("{Control>}{Enter}{/Control}")

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${task.id}`]))
  })

  it("renders the brief as Markdown in Preview, and hands the text back on Write", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(
      screen.getByLabelText("Description"),
      "# Wire it{Enter}{Enter}- one{Enter}- two",
    )
    await user.click(screen.getByRole("tab", { name: "Preview" }))

    const preview = screen.getByRole("tabpanel")
    expect(within(preview).getByRole("heading", { name: "Wire it" })).toBeDefined()
    expect(within(preview).getAllByRole("listitem")).toHaveLength(2)
    // The box is a view of the value, not a copy: it is gone while previewing.
    expect(screen.queryByLabelText("Description")).toBeNull()

    await user.click(screen.getByRole("tab", { name: "Write" }))

    expect((screen.getByLabelText("Description") as HTMLTextAreaElement).value).toBe(
      "# Wire it\n\n- one\n- two",
    )
  })
})
