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
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
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
  extra_flags: [],
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

  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  return render(
    <MemoryRouter initialEntries={[paths.goals()]}>
      <QueryClientProvider client={queryClient}>
        <Where />
        <CreateGoalDialog open onOpenChange={() => {}} />
      </QueryClientProvider>
    </MemoryRouter>,
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
    renderDialog()

    expect(await screen.findByRole("checkbox", { name: new RegExp(ARIADNE.path) })).toBeDefined()
    expect(screen.getByRole("checkbox", { name: new RegExp(SANDBOX.path) })).toBeDefined()
    expect(screen.getByText("main")).toBeDefined()
    expect(screen.getByText("trunk")).toBeDefined()
    expect(screen.getByText("The orchestrator itself.")).toBeDefined()
    // Select-only: nothing here types a path.
    expect(screen.queryByPlaceholderText("/absolute/path/to/repo")).toBeNull()
  })

  it("submits the ids of what was ticked, and nothing about the paths", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Title"), "Repositories")
    await user.click(await screen.findByRole("checkbox", { name: new RegExp(ARIADNE.path) }))
    await user.click(screen.getByRole("checkbox", { name: new RegExp(SANDBOX.path) }))
    await user.click(screen.getByRole("button", { name: "Create goal" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/goals" })
    expect(lastWrite()?.body?.repository_ids).toEqual([ARIADNE.id, SANDBOX.id])
  })

  it("untickng takes one back off, so the body follows the boxes", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Title"), "Repositories")
    const ariadne = await screen.findByRole("checkbox", { name: new RegExp(ARIADNE.path) })
    await user.click(ariadne)
    await user.click(screen.getByRole("checkbox", { name: new RegExp(SANDBOX.path) }))
    await user.click(ariadne)
    await user.click(screen.getByRole("button", { name: "Create goal" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.repository_ids).toEqual([SANDBOX.id])
  })

  it("will not create a goal against none of them", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Title"), "Repositories")
    await screen.findByRole("checkbox", { name: new RegExp(ARIADNE.path) })
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
