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
 * dialog: one control per slot, holding the whole choice — the agent CLI, the
 * model of it and the effort it is run at — which pick lands in which slot when
 * there are several of them, and — on an edit — what an untouched pin is
 * measured against when the profiles that decide how it was seeded only land
 * after the user has started typing.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { GoalDto, ModelDto, SkillDto } from "@/api"
import { aGoal, aModel, anEffort, aSkill } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { CreateTaskDialog, EditTaskDialog } from "./task-form-dialog"

const STAMP = "2026-01-01T00:00:00Z"

const GOAL: GoalDto = aGoal()

const CODING: SkillDto = aSkill({ name: "coding" })

const REVIEWING: SkillDto = {
  ...CODING,
  name: "code-review",
}

/** A second one, for the edit that replaces the task's reviewer list. */
const STRICT_REVIEWING: SkillDto = { ...CODING, name: "security-review" }

/** The catalog every model box offers, whole, with the efforts of each entry. */
const CATALOG: ModelDto[] = [
  aModel({
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
    tier: "strong",
    efforts: [
      anEffort({ id: "low" }),
      anEffort({ id: "medium" }),
      anEffort({ id: "high", default: true }),
      anEffort({ id: "xhigh" }),
      anEffort({ id: "max" }),
    ],
  }),
  aModel({
    // The one that takes none at all, which is what disables the field.
    id: "claude_code:claude-haiku-4-5",
    agent_kind: "claude_code",
    description: "Fast and cheap",
    tier: "fast",
  }),
  aModel({
    id: "codex:gpt-5.3-codex",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
    tier: "frontier",
    efforts: [
      anEffort({ id: "low" }),
      anEffort({ id: "medium", default: true }),
      anEffort({ id: "high" }),
      anEffort({ id: "xhigh" }),
      anEffort({ id: "ultra" }),
    ],
  }),
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

    if (url.pathname === "/v1/skills") {
      switch (url.searchParams.get("seat")) {
        case "reviewer":
          return answer([REVIEWING])
        default:
          return answer([CODING])
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

/** The named slot's pin control: one per slot, and the whole choice for it. */
async function pinButton(slot: string): Promise<HTMLElement> {
  return await screen.findByRole("button", { name: `${slot} runs on` })
}

/** What a slot's trigger reads, with the whitespace a screen collapses collapsed. */
async function pinReads(slot: string): Promise<string> {
  return ((await pinButton(slot)).textContent ?? "").replace(/\s+/g, " ").trim()
}

/** Opens a slot's picker, and answers the catalog inside it. */
async function openPin(
  user: ReturnType<typeof userEvent.setup>,
  slot: string,
): Promise<HTMLElement> {
  await user.click(await pinButton(slot))
  return await screen.findByRole("listbox", { name: "Models" })
}

/** Shuts whichever picker is open, leaving the dialog behind it up. */
async function closePin(user: ReturnType<typeof userEvent.setup>): Promise<void> {
  await user.keyboard("{Escape}")
  await vi.waitFor(() => expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull())
}

/** Picks `model` out of the catalog in the named slot's picker. */
async function pickModel(
  user: ReturnType<typeof userEvent.setup>,
  slot: string,
  model: string,
): Promise<void> {
  const models = await openPin(user, slot)
  await user.click(within(models).getByText(model))
  await closePin(user)
}

/** Pins a model the catalog does not carry, which is typed rather than picked. */
async function typeModel(
  user: ReturnType<typeof userEvent.setup>,
  slot: string,
  id: string,
): Promise<void> {
  await openPin(user, slot)
  await user.type(screen.getByRole("combobox", { name: `${slot} runs on` }), id)
  await user.click(screen.getByText(/^Other — run/))
  await closePin(user)
}

/** Picks `effort` off the strip under the catalog, for whatever the slot is on. */
async function pickEffort(
  user: ReturnType<typeof userEvent.setup>,
  slot: string,
  effort: string,
): Promise<void> {
  await openPin(user, slot)
  await user.click(await screen.findByRole("radio", { name: effort }))
  await closePin(user)
}

describe("dismissing the dialog", () => {
  it("closes an untouched form straight away", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(onOpenChange)

    // The preselects are what this is about: wait until they have happened.
    expect(await screen.findByLabelText("Author skills")).toBeDefined()

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
    agents: [
      { id: "01AGENTAUTHOR", seat: "author", skills: ["coding"] },
      { id: "01AGENTREVIEW", seat: "reviewer", skills: ["code-review"] },
    ],
  }

  /** The same task, with its author pinned. */
  const PINNED_TASK = {
    ...TASK,
    agents: [
      {
        id: "01AGENTAUTHOR",
        seat: "author" as const,
        skills: ["coding"],
        model: "claude_code:claude-opus-5",
      },
      { id: "01AGENTREVIEW", seat: "reviewer" as const, skills: ["code-review"] },
    ],
  }

  /**
   * The same task on another CLI than its author profile's, and on that
   * CLI's own default model rather than a named one.
   */
  const CODEX_TASK = {
    ...TASK,
    agents: [
      { id: "01AGENTAUTHOR", seat: "author" as const, skills: ["coding"], model: "codex" },
      { id: "01AGENTREVIEW", seat: "reviewer" as const, skills: ["code-review"] },
    ],
  }

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
      if (url.pathname === "/v1/skills") return answer([REVIEWING, STRICT_REVIEWING])
      if (url.pathname === "/v1/tasks") return answer([])
      return new Response("not stubbed", { status: 404 })
    })
    renderEdit()

    // The task's own reviewer is what the row starts on, not a default.
    expect(await screen.findByLabelText("Reviewer 1 skills")).toBeDefined()

    const box = await screen.findByLabelText("Reviewer 1 skills")
    await user.clear(box)
    await user.type(box, "security-review")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).toMatchObject({
      reviewers: [{ seat: "reviewer", skills: ["security-review"] }],
    })
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
  it("says nothing about a model box nobody touched", async () => {
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
      if (url.pathname === "/v1/skills") {
        await profiles
        return jsonResponse([CODING, REVIEWING])
      }
      if (url.pathname === "/v1/models") return jsonResponse(CATALOG)
      if (url.pathname === "/v1/tasks") return jsonResponse([])
      return new Response("not stubbed", { status: 404 })
    })
    renderScreen(<EditTaskDialog task={PINNED_TASK as never} open onOpenChange={vi.fn()} />)

    // Nothing to read the pin against yet, so the pin is what the trigger says.
    expect(await pinReads("Author")).toBe("Claude Code claude-opus-5")

    await user.type(screen.getByLabelText("Title"), "!")
    landProfiles()

    // A reviewer row shows its profile's name only once the profiles are in,
    // so it is the signal — and the pin is still what the user saw.
    expect(await screen.findByLabelText("Reviewer 1 skills")).toBeDefined()
    expect(await pinReads("Author")).toBe("Claude Code claude-opus-5")

    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).not.toHaveProperty("model")
  })

  it("opens on the pinned agent CLI alone, which is that CLI's default model", async () => {
    renderEdit(CODEX_TASK)

    // The author profile runs on claude_code, so the pin is an override and
    // shows as itself — the CLI on its own, with no model after it.
    expect(await pinReads("Author")).toBe("Codex default model")
  })

  it("sends the daemon's sentinel when a pin is emptied back to auto", async () => {
    const user = userEvent.setup()
    renderEdit(CODEX_TASK)

    const models = await openPin(user, "Author")
    await user.click(within(models).getByText(/^auto/))
    await closePin(user)
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).toMatchObject({ model: "default" })
  })

  it("says nothing about a pin left alone, whatever else was edited", async () => {
    const user = userEvent.setup()
    renderEdit(CODEX_TASK)

    expect(await pinReads("Author")).toBe("Codex default model")
    await user.type(screen.getByLabelText("Title"), "!")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await vi.waitFor(() => expect(writes).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(posted[0]).not.toHaveProperty("model")
  })
})

/**
 * One pin per slot, chosen on the form that assigns them: one control, holding
 * the agent CLI, the model of it and the effort it is run at.
 */
describe("what the task's agents run on", () => {
  it("offers every slot the catalog whole, grouped by agent CLI", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    const models = await openPin(user, "Reviewer 1")
    expect(within(models).getByText("codex:gpt-5.3-codex")).toBeDefined()
    expect(within(models).getByText("claude_code:claude-opus-5")).toBeDefined()
    expect(within(models).getByText("Codex")).toBeDefined()

    await closePin(user)
  })

  /**
   * Most work is worth a second pair of eyes, and the form starts with one
   * reviewer for that reason. Some work has nothing to review — a release,
   * a dependency bump the suite already judged — and that task is staffed
   * with an author alone.
   */
  it("takes every reviewer off, for a task with nothing to review", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Cut 0.6.0")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    await user.click(screen.getByRole("button", { name: "Remove reviewer 1" }))

    expect(screen.queryByLabelText("Reviewer 1 skills")).toBeNull()
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({ agents: [{ seat: "author", skills: ["coding"] }] })
  })

  it("is three controls on a reviewer row: the skills, what it runs on, and the remove", async () => {
    renderDialog(vi.fn())

    // The built-in Reviewer with nothing of its own pinned: the row says what
    // it will actually run on rather than leaving the choice blank.
    expect(await screen.findByLabelText("Reviewer 1 skills")).toBeDefined()
    await vi.waitFor(async () => expect(await pinReads("Reviewer 1")).toContain("auto"))
    expect(screen.getByRole("button", { name: "Remove reviewer 1" })).toBeDefined()
  })

  it("sends the author's model, and each reviewer's, from its own box", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    await pickModel(user, "Author", "codex:gpt-5.3-codex")
    await pickModel(user, "Reviewer 1", "claude_code:claude-opus-5")

    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({
      agents: [
        { seat: "author", skills: ["coding"], model: "codex:gpt-5.3-codex" },
        { seat: "reviewer", skills: ["code-review"], model: "claude_code:claude-opus-5" },
      ],
    })
  })

  it("sends an agent CLI typed on its own, which is that CLI's default", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    await typeModel(user, "Author", "codex")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({
      agents: [
        { seat: "author", skills: ["coding"], model: "codex" },
        { seat: "reviewer", skills: ["code-review"] },
      ],
    })
  })

  it("keeps an id whose model half carries colons of its own", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    await typeModel(user, "Author", "opencode:ollama/llama3:8b")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({
      agents: [
        { seat: "author", skills: ["coding"], model: "opencode:ollama/llama3:8b" },
        { seat: "reviewer", skills: ["code-review"] },
      ],
    })
  })

  it("refuses a model naming no agent CLI, on the row it was typed in", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    await typeModel(user, "Reviewer 1", "claude-opus-5")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    expect(await screen.findByText(/claude_code:claude-opus-5/)).toBeDefined()
    expect(writes).toEqual([])
  })

  it("leaves an untouched agent out, which runs it on auto", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({
      agents: [
        { seat: "author", skills: ["coding"] },
        { seat: "reviewer", skills: ["code-review"] },
      ],
    })
    const staffed = (posted[0] as { agents: Record<string, unknown>[] }).agents
    expect(staffed[0]).not.toHaveProperty("model")
  })
})

/**
 * The effort strip under each slot's catalog: offered from what that slot's
 * model can be run at, and cleared back to the CLI's own when the model moves
 * to one that does not take it — which is what the daemon does with it.
 */
describe("what each agent is run at", () => {
  it("sends the author's effort, and each reviewer's, from its own box", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    await pickModel(user, "Author", "codex:gpt-5.3-codex")
    await pickEffort(user, "Author", "ultra")
    await pickModel(user, "Reviewer 1", "claude_code:claude-opus-5")
    await pickEffort(user, "Reviewer 1", "max")

    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({
      agents: [
        { seat: "author", skills: ["coding"], model: "codex:gpt-5.3-codex", effort: "ultra" },
        {
          seat: "reviewer",
          skills: ["code-review"],
          model: "claude_code:claude-opus-5",
          effort: "max",
        },
      ],
    })
  })

  /**
   * An effort with no model beside it is a pin of its own: the daemon runs the
   * model the slot would have run on anyway — the profile's — at that effort
   * (`http/pins.rs`, `chosen`), which is what `pinFields` sends it as.
   */
  it("offers no effort until a model is chosen, since an effort belongs to one", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()

    // An agent on auto has no model, and there was never anything behind it
    // to borrow one from — so the strip has nothing to offer, and the daemon
    // never sees an effort it would have to refuse.
    await openPin(user, "Reviewer 1")
    expect(screen.queryAllByRole("radio")).toHaveLength(0)
  })

  it("offers only what the slot's own model takes", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    const models = await openPin(user, "Author")
    await user.click(within(models).getByText("claude_code:claude-opus-5"))

    // The claude entry's list, named with what that CLI runs it at — not the
    // codex one beside it in the catalog.
    expect(await screen.findByRole("radio", { name: "auto (high)" })).toBeDefined()
    expect(screen.queryByRole("radio", { name: "ultra" })).toBeNull()

    // Closed again before the test ends: an open popup holds the pointer over
    // the whole document, which is not a state to hand to the next test.
    await closePin(user)
  })

  it("drops an effort back to auto when the model moves to one that takes none", async () => {
    const user = userEvent.setup()
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    const models = await openPin(user, "Author")
    await user.click(within(models).getByText("claude_code:claude-opus-5"))
    await user.click(await screen.findByRole("radio", { name: "max" }))

    // Picked over rather than handed back: the model moves, and the effort it
    // belonged to does not come with it.
    await user.click(within(models).getByText("claude_code:claude-haiku-4-5"))

    await vi.waitFor(() => expect(screen.queryAllByRole("radio")).toHaveLength(0))
    expect(screen.getByText(/takes no effort/)).toBeDefined()
    await closePin(user)

    await user.click(screen.getByRole("button", { name: "Create task" }))

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    const author = (posted[0] as { agents: Record<string, unknown>[] }).agents[0]
    expect(author).toMatchObject({ model: "claude_code:claude-haiku-4-5" })
    expect(author).not.toHaveProperty("effort")
  })

  it("shows the daemon's refusal of an effort, with the dialog still up", async () => {
    const user = userEvent.setup()
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      if (request.method !== "GET") {
        writes.push(`${request.method} ${url.pathname}`)
        return errorResponse(
          400,
          "bad_request",
          "`ultra` is no effort of that model — it takes low, medium, high, xhigh",
        )
      }
      if (url.pathname === "/v1/skills") {
        return jsonResponse([CODING, REVIEWING])
      }
      if (url.pathname === "/v1/models") return jsonResponse(CATALOG)
      if (url.pathname === "/v1/tasks") return jsonResponse([])
      return new Response("not stubbed", { status: 404 })
    })
    renderDialog(vi.fn())

    await user.type(screen.getByLabelText("Title"), "Wire the strip")
    expect(await screen.findByLabelText("Author skills")).toBeDefined()
    await pickModel(user, "Author", "codex:gpt-5.3-codex")
    await pickEffort(user, "Author", "ultra")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    expect(await screen.findByText(/is no effort of that model/)).toBeDefined()
    expect(screen.getByRole("button", { name: "Create task" })).toBeDefined()
  })
})

/**
 * The brief is what the author builds from and it is read as Markdown, so
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
    expect(await screen.findByLabelText("Author skills")).toBeDefined()

    await user.type(screen.getByLabelText("Description"), "Wire the strip")
    await user.keyboard("{Meta>}{Enter}{/Meta}")

    await vi.waitFor(() => expect(writes).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(posted[0]).toMatchObject({ title: "Wire the strip", description: "Wire the strip" })
  })

  it("saves an edited task on Ctrl+Enter too", async () => {
    const user = userEvent.setup()
    const task = {
      ...CREATED,
      agents: [
        { id: "01AGENTAUTHOR", seat: "author", skills: ["coding"] },
        { id: "01AGENTREVIEW", seat: "reviewer", skills: ["code-review"] },
      ],
    }
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
