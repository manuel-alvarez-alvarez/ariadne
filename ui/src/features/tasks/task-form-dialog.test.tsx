// @vitest-environment jsdom

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { GoalDto, ModelDto, SkillDto, TaskDto } from "@/api"
import { aGoal, aModel, anEffort, aSkill, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { CreateTaskDialog, EditTaskDialog } from "./task-form-dialog"

const GOAL: GoalDto = aGoal()
const CODING: SkillDto = aSkill({ name: "coding" })
const REVIEWING: SkillDto = aSkill({ name: "code-review" })
/** The orchestrator's own playbook: not a task staffing choice. */
const ORCHESTRATION: SkillDto = aSkill({
  name: "orchestration",
  seat: "orchestrator",
  summary: "Plan a goal.",
})
const CATALOG: ModelDto[] = [
  aModel({
    id: "codex-acp:gpt-5.6",
    agent_id: "codex-acp",
    efforts: [anEffort({ id: "high", default: true })],
  }),
  aModel({
    id: "claude-code-acp:claude-sonnet-5",
    agent_id: "claude-code-acp",
    efforts: [anEffort({ id: "high", default: true })],
  }),
  aModel({ id: "claude-code-acp:claude-haiku-4-5", agent_id: "claude-code-acp" }),
]

let writes: unknown[]
let writePaths: string[]

const TASK: TaskDto = aTask({
  goal_id: GOAL.id,
  status: "pending",
  agents: [
    { id: "01AGENTAUTHOR", seat: "author", skills: ["coding"], model: "codex-acp:gpt-5.6" },
    {
      id: "01AGENTREVIEW",
      seat: "reviewer",
      skills: ["code-review"],
      model: "claude-code-acp:claude-sonnet-5",
    },
  ],
})

beforeEach(() => {
  writes = []
  writePaths = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    if (request.method !== "GET") {
      writePaths.push(`${request.method} ${pathname}`)
      writes.push(await request.clone().json())
    }
    if (pathname === "/v1/skills") return jsonResponse([CODING, REVIEWING, ORCHESTRATION])
    if (pathname === "/v1/models") return jsonResponse(CATALOG)
    if (pathname === "/v1/tasks") return jsonResponse([])
    if (request.method === "PATCH" && pathname === `/v1/tasks/${TASK.id}`) return jsonResponse(TASK)
    if (pathname === `/v1/goals/${GOAL.id}/tasks`) {
      return jsonResponse({
        id: "01JTASK0000000000000000001",
        goal_id: GOAL.id,
        repo_id: GOAL.repos[0]?.id ?? "01JREPO0000000000000000001",
        title: "Task",
        description: "",
        status: "pending",
        branch: "task-000001",
        landing: "merge",
        agents: [],
        depends_on: [],
        stalled: false,
        usage: {
          total: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
          author: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
          reviewers: [],
        },
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      })
    }
    return new Response("not stubbed", { status: 404 })
  })
})

function renderDialog() {
  renderScreen(<CreateTaskDialog goal={GOAL} open onOpenChange={vi.fn()} />)
}

function renderEdit(task: TaskDto = TASK, onOpenChange = vi.fn()) {
  renderScreen(<EditTaskDialog task={task} open onOpenChange={onOpenChange} />)
  return onOpenChange
}

/** The names a skills box suggests, read off the `<datalist>` behind it. */
function suggestionsOf(input: HTMLInputElement): (string | null)[] {
  const list = document.getElementById(input.getAttribute("list") ?? "")
  return [...(list?.children ?? [])].map((option) => option.getAttribute("value"))
}

async function pickModel(user: ReturnType<typeof userEvent.setup>, slot: string, model: string) {
  await user.click(await screen.findByRole("button", { name: `${slot} runs on` }))
  const options = await screen.findByRole("listbox", { name: "Models" })
  await user.click(within(options).getByText(model))
  await user.keyboard("{Escape}")
}

async function openPicker(user: ReturnType<typeof userEvent.setup>, slot: string) {
  await user.click(await screen.findByRole("button", { name: `${slot} runs on` }))
  return screen.findByRole("listbox", { name: "Models" })
}

async function closePicker(user: ReturnType<typeof userEvent.setup>) {
  await user.keyboard("{Escape}")
}

it("disables create until the author and every reviewer have models", async () => {
  const user = userEvent.setup()
  renderDialog()

  const submit = screen.getByRole("button", { name: "Create task" }) as HTMLButtonElement
  expect(submit.disabled).toBe(true)
  await pickModel(user, "Author", "codex-acp:gpt-5.6")
  expect(submit.disabled).toBe(true)
  await pickModel(user, "Reviewer 1", "claude-code-acp:claude-sonnet-5")
  expect(submit.disabled).toBe(false)
})

it("sends each concrete model with the task staffing", async () => {
  const user = userEvent.setup()
  renderDialog()

  await user.type(screen.getByLabelText("Title"), "Demand a model")
  await pickModel(user, "Author", "codex-acp:gpt-5.6")
  await pickModel(user, "Reviewer 1", "claude-code-acp:claude-sonnet-5")
  await user.click(screen.getByRole("button", { name: "Create task" }))

  await waitFor(() => expect(writes).toHaveLength(1))
  expect(writes[0]).toMatchObject({
    agents: [
      { seat: "author", model: "codex-acp:gpt-5.6" },
      { seat: "reviewer", model: "claude-code-acp:claude-sonnet-5" },
    ],
  })
})

it("refuses a bare agent before it sends the task", async () => {
  const user = userEvent.setup()
  renderDialog()

  await user.type(screen.getByLabelText("Title"), "Demand a model")
  await pickModel(user, "Author", "codex-acp:gpt-5.6")
  await user.click(await screen.findByRole("button", { name: "Reviewer 1 runs on" }))
  await user.type(screen.getByRole("combobox", { name: "Reviewer 1 runs on" }), "claude-code-acp")
  await user.click(screen.getByText(/^Other — run/))
  await user.keyboard("{Escape}")
  await user.click(screen.getByRole("button", { name: "Create task" }))

  expect(await screen.findByText(/claude-code-acp:<model>/)).toBeDefined()
  expect(writes).toEqual([])
})

describe("editing a pending task", () => {
  it("patches reviewers with their concrete models", async () => {
    const user = userEvent.setup()
    renderEdit()

    const skills = await screen.findByLabelText("Reviewer 1 skills")
    await user.clear(skills)
    await user.type(skills, "security-review")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => expect(writePaths).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(writes[0]).toMatchObject({
      reviewers: [
        { seat: "reviewer", skills: ["security-review"], model: "claude-code-acp:claude-sonnet-5" },
      ],
    })
  })

  it("does not resend an untouched author pin after the catalog arrives", async () => {
    const user = userEvent.setup()
    let releaseCatalog = () => {}
    const catalog = new Promise<void>((resolve) => {
      releaseCatalog = resolve
    })
    let catalogAnswered = false
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const { pathname } = new URL(request.url)
      if (request.method !== "GET") {
        writePaths.push(`${request.method} ${pathname}`)
        writes.push(await request.clone().json())
      }
      if (pathname === "/v1/skills") return jsonResponse([CODING, REVIEWING])
      if (pathname === "/v1/models") {
        await catalog
        catalogAnswered = true
        return jsonResponse(CATALOG)
      }
      if (pathname === "/v1/tasks") return jsonResponse([])
      if (request.method === "PATCH" && pathname === `/v1/tasks/${TASK.id}`)
        return jsonResponse(TASK)
      return new Response("not stubbed", { status: 404 })
    })
    renderEdit()

    await user.type(screen.getByLabelText("Title"), "!")
    releaseCatalog()
    await waitFor(() => expect(catalogAnswered).toBe(true))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => expect(writePaths).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
    expect(writes[0]).not.toHaveProperty("model")
    expect(writes[0]).not.toHaveProperty("effort")
  })

  it("saves an edited task with Ctrl+Enter", async () => {
    const user = userEvent.setup()
    renderEdit()

    await user.type(await screen.findByLabelText("Description"), "One more thing")
    await user.keyboard("{Control>}{Enter}{/Control}")

    await waitFor(() => expect(writePaths).toEqual([`PATCH /v1/tasks/${TASK.id}`]))
  })
})

describe("the rest of the task form", () => {
  it("suggests no orchestrator-only skill for the author or a reviewer", async () => {
    renderDialog()

    const author = (await screen.findByLabelText("Author skills")) as HTMLInputElement
    const reviewer = (await screen.findByLabelText("Reviewer 1 skills")) as HTMLInputElement
    expect(suggestionsOf(author)).toEqual(["coding", "code-review"])
    expect(suggestionsOf(reviewer)).toEqual(["coding", "code-review"])
  })

  it("asks before discarding a dirty draft and keeps it when dismissed", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Description"), "Rewrite the scheduler.")
    await user.keyboard("{Escape}")

    expect(await screen.findByText("Discard changes?")).toBeDefined()
    await user.click(screen.getByRole("button", { name: "Keep editing" }))
    expect((screen.getByLabelText("Description") as HTMLTextAreaElement).value).toBe(
      "Rewrite the scheduler.",
    )
  })

  it("renders the brief as Markdown in Preview and restores it in Write", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(
      screen.getByLabelText("Description"),
      "# Wire it{Enter}{Enter}- one{Enter}- two",
    )
    await user.click(screen.getByRole("tab", { name: "Preview" }))

    const preview = screen.getByRole("tabpanel")
    expect(within(preview).getByRole("heading", { name: "Wire it" })).toBeDefined()
    expect(within(preview).getAllByRole("listitem")).toHaveLength(2)
    await user.click(screen.getByRole("tab", { name: "Write" }))
    expect((screen.getByLabelText("Description") as HTMLTextAreaElement).value).toBe(
      "# Wire it\n\n- one\n- two",
    )
  })

  it("sends the selected landing choice", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Title"), "Do not land this")
    await pickModel(user, "Author", "codex-acp:gpt-5.6")
    await pickModel(user, "Reviewer 1", "claude-code-acp:claude-sonnet-5")
    await user.click(screen.getByRole("combobox", { name: "Ends with" }))
    await user.click(await screen.findByRole("option", { name: "Land nothing" }))
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await waitFor(() => expect(writePaths).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    expect(writes[0]).toMatchObject({ landing: "none" })
  })

  it("drops effort when its model moves to one that takes none", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Title"), "Move the model")
    const models = await openPicker(user, "Author")
    await user.click(within(models).getByText("claude-code-acp:claude-sonnet-5"))
    await user.click(await screen.findByRole("radio", { name: "high" }))
    await user.click(within(models).getByText("claude-code-acp:claude-haiku-4-5"))
    await closePicker(user)
    await pickModel(user, "Reviewer 1", "codex-acp:gpt-5.6")
    await user.click(screen.getByRole("button", { name: "Create task" }))

    await waitFor(() => expect(writePaths).toEqual([`POST /v1/goals/${GOAL.id}/tasks`]))
    const author = (writes[0] as { agents: Record<string, unknown>[] }).agents[0]
    expect(author).toMatchObject({ model: "claude-code-acp:claude-haiku-4-5" })
    expect(author).not.toHaveProperty("effort")
  })
})
