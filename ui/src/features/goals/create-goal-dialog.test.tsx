// @vitest-environment jsdom

/**
 * The goal dialog's repository field, which is the half of the form the daemon
 * used to validate.
 *
 * A goal is no longer created from typed paths but from checkouts that are
 * already registered, so what is asserted here is what that swap has to
 * guarantee: only registered repositories are offered, the ids of the picked
 * ones are what goes on the wire, a goal cannot be created against none of
 * them, and — the case a fresh install lands in — an empty registry offers the
 * way to fill it rather than an empty box with nothing to click.
 *
 * The registry being unbounded, the picker is a searchable combobox rather
 * than a list of checkboxes, so the same guarantees are asserted through it:
 * the search narrows what is offered, several picks are made without the popup
 * closing between them, and each one can be taken back off from the chip it
 * left behind.
 *
 * What the planner runs on is the other field with a rule of its own, and it
 * is one: a model that names the agent CLI running it, on the wire when it was
 * filled in and left out entirely when it was not — which is what runs the
 * planner on its profile's own. A model naming no CLI never reaches the
 * daemon: the field refuses it first.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useLocation } from "react-router-dom"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { ModelDto, ProfileDto, RepositoryDto } from "@/api"
import { paths } from "@/routes/paths"
import { aProfile, aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { CreateGoalDialog } from "./create-goal-dialog"

const PLANNER: ProfileDto = aProfile({
  id: "01JPROF00000000000000PLN",
  name: "Planner",
  role: "planner",
})

const ARIADNE: RepositoryDto = aRepository({
  id: "01JREPO00000000000000ARI",
})

/** Two agents' worth of catalog, which the picker offers whole. */
const CATALOG: ModelDto[] = [
  {
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
  },
  {
    id: "codex:gpt-5.3-codex",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
  },
]

const SANDBOX: RepositoryDto = aRepository({
  id: "01JREPO00000000000000SND",
  path: "/home/me/dev/sandbox",
  base_branch: "trunk",
  description: null,
})

interface Recorded {
  method: string
  path: string
  body: {
    repository_ids?: string[]
    title?: string
    model?: string
  } | null
}

let requests: Recorded[] = []

function lastWrite(): Recorded | undefined {
  return writes().at(-1)
}

/** The form's submit, which goes out of reach while a create is in flight. */
function submitButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "Create goal" }) as HTMLButtonElement
}

/** Everything the dialog sent that was not a read. */
function writes(): Recorded[] {
  return requests.filter((one) => one.method !== "GET")
}

function stubDaemon(repositories: RepositoryDto[]) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (pathname === "/v1/repositories") return jsonResponse(repositories)
    if (pathname === "/v1/models") return jsonResponse(CATALOG)
    if (pathname === "/v1/profiles") return jsonResponse([PLANNER])
    if (pathname === "/v1/goals" && request.method === "POST") {
      return jsonResponse(
        { ...body, id: "01JGOAL0000000000000NEW", repos: repositories, status: "planning" },
        201,
      )
    }
    return new Response("not stubbed", { status: 404 })
  })
}

/** The dialog, with the route it can navigate to shown next to it. */
function renderDialog() {
  function Where() {
    return <span data-testid="where">{useLocation().pathname}</span>
  }

  const onOpenChange = vi.fn()
  renderScreen(
    <>
      <Where />
      <CreateGoalDialog open onOpenChange={onOpenChange} />
    </>,
    { route: paths.goals() },
  )
  return { onOpenChange }
}

/** The field itself, which is what the popup hangs off. */
async function repositoryBox(): Promise<HTMLElement> {
  return await screen.findByRole("combobox", { name: "Repositories" })
}

/** The list of repositories, which lives in a portal outside the dialog. */
async function options(): Promise<HTMLElement> {
  return await screen.findByRole("listbox", { name: "Repositories" })
}

/** Opens the popup and answers the list inside it. */
async function openList(user: ReturnType<typeof userEvent.setup>): Promise<HTMLElement> {
  await user.click(await repositoryBox())
  return await options()
}

/** One repository's row, found the way it reads: by its path. */
function row(list: HTMLElement, repository: RepositoryDto): HTMLElement {
  return within(list).getByRole("option", { name: new RegExp(repository.path) })
}

beforeEach(() => {
  requests = []
  stubDaemon([ARIADNE, SANDBOX])
})

describe("picking the goal's repositories", () => {
  it("offers the registered ones, with the branch and the description beside each", async () => {
    const user = userEvent.setup()
    renderDialog()

    const list = await openList(user)

    expect(row(list, ARIADNE)).toBeDefined()
    expect(row(list, SANDBOX)).toBeDefined()
    expect(within(list).getByText("main")).toBeDefined()
    expect(within(list).getByText("trunk")).toBeDefined()
    expect(within(list).getByText("The orchestrator itself.")).toBeDefined()
    // Select-only: nothing here types a path.
    expect(screen.queryByPlaceholderText("/absolute/path/to/repo")).toBeNull()
  })

  it("narrows the list to what is typed, and picks out of what is left", async () => {
    const user = userEvent.setup()
    renderDialog()

    const list = await openList(user)
    await user.type(screen.getByRole("combobox", { name: "Search repositories" }), "sandbox")

    await waitFor(() => {
      expect(within(list).queryByRole("option", { name: new RegExp(ARIADNE.path) })).toBeNull()
    })
    await user.click(row(list, SANDBOX))

    // The pick lands in the field, and the popup is still up for the next one.
    expect(screen.getByRole("button", { name: `Remove ${SANDBOX.path}` })).toBeDefined()
    expect(await options()).toBeDefined()
  })

  it("submits the ids of what was picked, and nothing about the paths", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Title"), "Repositories")
    const list = await openList(user)
    await user.click(row(list, ARIADNE))
    await user.click(row(list, SANDBOX))
    await user.keyboard("{Escape}")
    await user.click(screen.getByRole("button", { name: "Create goal" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/goals" })
    expect(lastWrite()?.body?.repository_ids).toEqual([ARIADNE.id, SANDBOX.id])
  })

  it("takes one back off from its chip, so the body follows the field", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Title"), "Repositories")
    const list = await openList(user)
    await user.click(row(list, ARIADNE))
    await user.click(row(list, SANDBOX))
    await user.keyboard("{Escape}")

    await user.click(screen.getByRole("button", { name: `Remove ${ARIADNE.path}` }))
    expect(screen.queryByRole("button", { name: `Remove ${ARIADNE.path}` })).toBeNull()

    await user.click(screen.getByRole("button", { name: "Create goal" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.repository_ids).toEqual([SANDBOX.id])
  })

  it("closes the list on Escape without taking the dialog down with it", async () => {
    const user = userEvent.setup()
    const { onOpenChange } = renderDialog()

    await openList(user)
    await user.keyboard("{Escape}")

    await waitFor(() => {
      expect(screen.queryByRole("listbox", { name: "Repositories" })).toBeNull()
    })
    expect(onOpenChange).not.toHaveBeenCalled()
  })

  it("will not create a goal against none of them", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Title"), "Repositories")
    await repositoryBox()
    await user.click(screen.getByRole("button", { name: "Create goal" }))

    expect(await screen.findByText("Pick at least one repository.")).toBeDefined()
    expect(requests.filter((one) => one.method !== "GET")).toEqual([])
  })
})

describe("with nothing registered", () => {
  it("sends the user to the screen that registers one instead of showing an empty box", async () => {
    const user = userEvent.setup()
    stubDaemon([])
    renderDialog()

    expect(await screen.findByText("No repositories registered")).toBeDefined()

    await user.click(screen.getByRole("link", { name: "Go to Repositories" }))

    expect(screen.getByTestId("where").textContent).toBe(paths.repositories())
  })
})

/**
 * The brief is the longest thing the app asks anyone to write, and the planner
 * form fills two of its own fields on open — a preselected planner profile is
 * the dialog's doing, not the user's, so it must not be what makes walking away
 * a question.
 */
describe("dismissing the dialog", () => {
  it("closes an untouched form straight away, preselected planner and all", async () => {
    const user = userEvent.setup()
    const { onOpenChange } = renderDialog()

    // The preselect is what this is about: wait until it has happened.
    expect(await screen.findByText("Planner")).toBeDefined()

    await user.click(screen.getByRole("button", { name: "Cancel" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })

  it("asks before dropping a typed brief, and keeps it when the answer is no", async () => {
    const user = userEvent.setup()
    const { onOpenChange } = renderDialog()

    const description = screen.getByLabelText("Description")
    await user.type(description, "Rewrite the scheduler.")
    await user.keyboard("{Escape}")

    expect(await screen.findByText("Discard changes?")).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Keep editing" }))

    expect((screen.getByLabelText("Description") as HTMLTextAreaElement).value).toBe(
      "Rewrite the scheduler.",
    )
    expect(onOpenChange).not.toHaveBeenCalled()
  })
})

/**
 * The planner is pinned with one field, whose value carries the agent CLI: it
 * goes on the wire as typed or picked, and an empty box is left out rather
 * than sent empty.
 */
describe("choosing what the planner runs on", () => {
  /** Fills the required fields, so the submit is about the pin alone. */
  async function fillRequired(user: ReturnType<typeof userEvent.setup>) {
    await user.type(screen.getByLabelText("Title"), "Model selector")
    const list = await openList(user)
    await user.click(row(list, ARIADNE))
    await user.keyboard("{Escape}")
  }

  /** The one field the choice is made in, catalog and all. */
  async function modelBox(): Promise<HTMLInputElement> {
    return (await screen.findByRole("combobox", { name: "Planner model" })) as HTMLInputElement
  }

  it("offers the catalog whole, grouped by the agent CLI each model runs on", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.click(await modelBox())

    const models = await screen.findByRole("listbox", { name: "Models" })
    expect(within(models).getByText("Codex")).toBeDefined()
    expect(within(models).getByText("codex:gpt-5.3-codex")).toBeDefined()
    expect(within(models).getByText("Claude Code")).toBeDefined()
    expect(within(models).getByText("claude_code:claude-opus-5")).toBeDefined()
  })

  it("sends the picked id, which names the CLI and the model together", async () => {
    const user = userEvent.setup()
    renderDialog()

    await fillRequired(user)
    // A codex model on the Planner profile: the pin is the slot's, not the
    // profile's.
    await user.click(await modelBox())
    const models = await screen.findByRole("listbox", { name: "Models" })
    await user.click(within(models).getByText("codex:gpt-5.3-codex"))

    await user.click(screen.getByRole("button", { name: "Create goal" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).toMatchObject({ model: "codex:gpt-5.3-codex" })
  })

  it("sends an agent CLI on its own, which is that CLI's own default model", async () => {
    const user = userEvent.setup()
    renderDialog()

    await fillRequired(user)
    await user.type(await modelBox(), "codex")
    await user.click(screen.getByRole("button", { name: "Create goal" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.model).toBe("codex")
  })

  it("sends no model when the planner is left on its profile's own", async () => {
    const user = userEvent.setup()
    renderDialog()

    await fillRequired(user)
    await user.click(screen.getByRole("button", { name: "Create goal" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).not.toHaveProperty("model")
  })

  it("refuses a model that names no agent CLI, before the daemon is asked", async () => {
    const user = userEvent.setup()
    renderDialog()

    await fillRequired(user)
    await user.type(await modelBox(), "claude-opus-5")
    await user.click(screen.getByRole("button", { name: "Create goal" }))

    expect(await screen.findByText(/claude_code:claude-opus-5/)).toBeDefined()
    expect(lastWrite()).toBeUndefined()
  })
})

/**
 * The brief is the longest thing anyone types into this app, and the planner
 * reads it as Markdown — so the box is written in and read back in place, and
 * the form can be finished without reaching for the mouse.
 */
describe("writing the goal's brief", () => {
  /** Everything the daemon needs besides the brief, so a submit can land. */
  async function fillRequired(user: ReturnType<typeof userEvent.setup>) {
    await user.type(screen.getByLabelText("Title"), "Keyboard")
    const list = await openList(user)
    await user.click(row(list, ARIADNE))
    await user.keyboard("{Escape}")
  }

  it("keeps a plain Enter in the brief a newline, and the form open", async () => {
    const user = userEvent.setup()
    renderDialog()

    const brief = screen.getByLabelText("Description") as HTMLTextAreaElement
    await user.type(brief, "Ship it{Enter}then say so")

    expect(brief.value).toBe("Ship it\nthen say so")
    expect(lastWrite()).toBeUndefined()
  })

  it("creates the goal on the chord, typed from inside the brief", async () => {
    const user = userEvent.setup()
    renderDialog()

    await fillRequired(user)
    await user.type(screen.getByLabelText("Description"), "Ship it")
    await user.keyboard("{Meta>}{Enter}{/Meta}")

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/goals" })
    expect(lastWrite()?.body).toMatchObject({ title: "Keyboard" })
  })

  it("creates the goal on Ctrl+Enter too, from a field that is not the brief", async () => {
    const user = userEvent.setup()
    renderDialog()

    await fillRequired(user)
    await user.click(screen.getByLabelText("Title"))
    await user.keyboard("{Control>}{Enter}{/Control}")

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/goals" })
  })

  it("will not start a second create while the first is still in flight", async () => {
    const user = userEvent.setup()
    let land = () => {}
    const held = new Promise<void>((resolve) => {
      land = resolve
    })
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const { pathname } = new URL(request.url)
      const raw = await request.text()
      requests.push({ method: request.method, path: pathname, body: raw ? JSON.parse(raw) : null })
      if (pathname === "/v1/repositories") return jsonResponse([ARIADNE])
      if (pathname === "/v1/models") return jsonResponse(CATALOG)
      if (pathname === "/v1/profiles") return jsonResponse([PLANNER])
      if (pathname === "/v1/goals" && request.method === "POST") {
        await held
        return jsonResponse({ id: "01JGOAL0000000000000NEW", repos: [ARIADNE] }, 201)
      }
      return new Response("not stubbed", { status: 404 })
    })
    renderDialog()

    await fillRequired(user)
    await user.click(screen.getByLabelText("Description"))
    await user.keyboard("{Meta>}{Enter}{/Meta}")
    await waitFor(() => {
      expect(writes()).toHaveLength(1)
    })

    // The submit button is out of reach while it spins; the chord has to be
    // too, or it would post the goal a second time.
    await user.keyboard("{Meta>}{Enter}{/Meta}")
    land()

    // Settled: the button is back within reach, so nothing is still in flight.
    await waitFor(() => {
      expect(submitButton().disabled).toBe(false)
    })
    expect(writes()).toHaveLength(1)
  })

  it("renders the brief as Markdown in Preview, and hands the text back on Write", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(
      screen.getByLabelText("Description"),
      "# Ship it{Enter}{Enter}- one{Enter}- two",
    )
    await user.click(screen.getByRole("tab", { name: "Preview" }))

    const preview = screen.getByRole("tabpanel")
    expect(within(preview).getByRole("heading", { name: "Ship it" })).toBeDefined()
    expect(within(preview).getAllByRole("listitem")).toHaveLength(2)
    // The box is a view of the value, not a copy: it is gone while previewing.
    expect(screen.queryByLabelText("Description")).toBeNull()

    await user.click(screen.getByRole("tab", { name: "Write" }))

    expect((screen.getByLabelText("Description") as HTMLTextAreaElement).value).toBe(
      "# Ship it\n\n- one\n- two",
    )
  })
})
