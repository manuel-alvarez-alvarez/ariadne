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
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter, useLocation } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ProfileDto, RepositoryDto } from "@/api"
import { paths } from "@/routes/paths"

import { CreateGoalDialog } from "./create-goal-dialog"

/** Hoisted for the reason the profiles tests give: the client takes its
 * `fetch` when `@/api` is imported, so a later stub is one it never sees. */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const STAMP = "2026-01-01T00:00:00Z"

const PLANNER: ProfileDto = {
  id: "01JPROF00000000000000PLN",
  name: "Planner",
  role: "planner",
  agent_kind: "claude_code",
  model: null,
  system_prompt: "",
  created_at: STAMP,
  updated_at: STAMP,
}

const ARIADNE: RepositoryDto = {
  id: "01JREPO00000000000000ARI",
  path: "/home/me/dev/ariadne",
  base_branch: "main",
  description: "The orchestrator itself.",
  created_at: STAMP,
  updated_at: STAMP,
}

const SANDBOX: RepositoryDto = {
  id: "01JREPO00000000000000SND",
  path: "/home/me/dev/sandbox",
  base_branch: "trunk",
  description: null,
  created_at: STAMP,
  updated_at: STAMP,
}

interface Recorded {
  method: string
  path: string
  body: { repository_ids?: string[]; title?: string } | null
}

let requests: Recorded[] = []

function lastWrite(): Recorded | undefined {
  return requests.filter((one) => one.method !== "GET").at(-1)
}

function stubDaemon(repositories: RepositoryDto[]) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    const answer = (payload: unknown, status = 200) =>
      new Response(JSON.stringify(payload), {
        status,
        headers: { "content-type": "application/json" },
      })

    if (pathname === "/v1/repositories") return answer(repositories)
    if (pathname === "/v1/profiles") return answer([PLANNER])
    if (pathname === "/v1/goals" && request.method === "POST") {
      return answer(
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
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  render(
    <MemoryRouter initialEntries={[paths.goals()]}>
      <QueryClientProvider client={queryClient}>
        <Where />
        <CreateGoalDialog open onOpenChange={onOpenChange} />
      </QueryClientProvider>
    </MemoryRouter>,
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
  Element.prototype.scrollIntoView = vi.fn()
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  requests = []
  daemonFetch.mockReset()
  stubDaemon([ARIADNE, SANDBOX])
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
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
