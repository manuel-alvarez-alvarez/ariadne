// @vitest-environment jsdom

/**
 * The repository dialog against a stubbed daemon, from both ends of the form.
 *
 * Almost nothing here is client-side validation, and that is the point: only
 * the daemon can open a checkout, resolve a branch and check it has commits,
 * so the dialog's job is to send the right body and to put the daemon's answer
 * where the user can act on it. Each test is one of the ways that must not
 * curdle — an omitted branch has to reach the daemon as *absent* (which is
 * what asks for the repo's current branch) rather than as an empty string,
 * clearing a description has to reach it as an empty one (which is what clears
 * it), and a 400 has to land on the field it is about instead of a banner that
 * says "bad request" over a form that looks fine.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { RepositoryDto } from "@/api"

import { RepositoryFormDialog } from "./repository-form-dialog"

/** Hoisted for the reason `repositories-page.test.tsx` gives: the client takes
 * its `fetch` when `@/api` is imported, so a later stub is one it never sees. */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const STAMP = "2026-01-01T00:00:00Z"

const REPOSITORY: RepositoryDto = {
  id: "01JREPO00000000000000ARI",
  path: "/home/me/dev/ariadne",
  base_branch: "main",
  merge_strategy: "direct",
  description: "The orchestrator itself.",
  created_at: STAMP,
  updated_at: STAMP,
}

interface Recorded {
  method: string
  path: string
  body: { path?: string; base_branch?: string | null; description?: string | null } | null
}

let requests: Recorded[] = []

/** The last write that went out, whatever it was. */
function lastWrite(): Recorded | undefined {
  return requests.filter((one) => one.method !== "GET").at(-1)
}

/** The daemon, echoing writes back — or refusing one, as `failure` says. */
function stubDaemon(failure?: { status: number; code: string; message: string }) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (failure) {
      const { status, ...error } = failure
      return new Response(JSON.stringify({ error }), {
        status,
        headers: { "content-type": "application/json" },
      })
    }
    return new Response(JSON.stringify({ ...REPOSITORY, ...body }), {
      status: request.method === "POST" ? 201 : 200,
      headers: { "content-type": "application/json" },
    })
  })
}

function renderDialog(
  repository: RepositoryDto | null,
  onOpenChange: (open: boolean) => void = () => {},
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  return render(
    <QueryClientProvider client={queryClient}>
      <RepositoryFormDialog open onOpenChange={onOpenChange} repository={repository} />
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
  requests = []
  daemonFetch.mockReset()
  stubDaemon()
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe("registering a repository", () => {
  it("sends an omitted branch as absent, which is what asks for the current one", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/repositories" })
    expect(lastWrite()?.body).toEqual({
      path: "/home/me/dev/new",
      base_branch: null,
      description: null,
      merge_strategy: "direct",
    })
  })

  it("registers a repository whose tasks are published for a human to merge", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByLabelText("Merge strategy"))
    await user.click(await screen.findByRole("option", { name: "Publish a pull or merge request" }))
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).toMatchObject({ merge_strategy: "pull_request" })
  })

  it("refuses a relative path itself, without asking the daemon", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "dev/relative")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    expect(await screen.findByText("The path must be absolute.")).toBeDefined()
    expect(requests.filter((one) => one.method !== "GET")).toEqual([])
  })

  it("puts a bad path back on the path field, in the daemon's own words", async () => {
    const user = userEvent.setup()
    stubDaemon({
      status: 400,
      code: "bad_request",
      message: "/home/me/dev/new is not a git work tree",
    })
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    // On the field, not in the banner above the buttons.
    const message = await screen.findByText(/is not a git work tree/)
    expect(message.closest("[data-slot=field]")?.textContent).toContain("Path")
  })

  it("puts an unknown branch on the branch field, which is the one to fix", async () => {
    const user = userEvent.setup()
    stubDaemon({
      status: 400,
      code: "bad_request",
      message: "branch nope does not exist in /home/me/dev/new",
    })
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.type(screen.getByLabelText("Base branch"), "nope")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    const message = await screen.findByText(/branch nope does not exist/)
    expect(message.closest("[data-slot=field]")?.textContent).toContain("Base branch")
  })

  it("shows the pair already being registered above the buttons, where no field is at fault", async () => {
    const user = userEvent.setup()
    stubDaemon({
      status: 409,
      code: "conflict",
      message: "/home/me/dev/new on main is already registered",
    })
    renderDialog(null)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    const alert = await screen.findByRole("alert")
    expect(alert.textContent).toContain("already registered")
  })
})

describe("editing a repository", () => {
  it("starts from what is stored, and sends the branch back with the rest", async () => {
    const user = userEvent.setup()
    renderDialog(REPOSITORY)

    expect((screen.getByLabelText("Path") as HTMLInputElement).value).toBe(REPOSITORY.path)
    expect((screen.getByLabelText("Description") as HTMLTextAreaElement).value).toBe(
      REPOSITORY.description,
    )

    await user.type(screen.getByLabelText("Description"), " Now with repositories.")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({
      method: "PUT",
      path: `/v1/repositories/${REPOSITORY.id}`,
    })
    expect(lastWrite()?.body).toEqual({
      path: REPOSITORY.path,
      base_branch: "main",
      merge_strategy: "direct",
      description: "The orchestrator itself. Now with repositories.",
    })
  })

  it("clears a description with the empty string the daemon spells it as", async () => {
    const user = userEvent.setup()
    renderDialog(REPOSITORY)

    await user.clear(screen.getByLabelText("Description"))
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body?.description).toBe("")
  })
})

/**
 * The form holds a path nobody enjoys typing twice, so an outside press is not
 * an instruction to delete it — but an untouched form is still a form the user
 * gets to walk away from without being asked anything.
 */
describe("dismissing the dialog", () => {
  it("closes an untouched form straight away", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(REPOSITORY, onOpenChange)

    await user.click(screen.getByRole("button", { name: "Cancel" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })

  it("asks before dropping what was typed, and keeps it when the answer is no", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(null, onOpenChange)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.keyboard("{Escape}")

    expect(await screen.findByText("Discard changes?")).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Keep editing" }))

    expect((screen.getByLabelText("Path") as HTMLInputElement).value).toBe("/home/me/dev/new")
    expect(onOpenChange).not.toHaveBeenCalled()
  })

  it("closes a saved form on the spot, with no draft left to ask about", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(null, onOpenChange)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Register repository" }))

    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false)
    })
    expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/repositories" })
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })

  it("closes and drops the draft once the discard is confirmed", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(null, onOpenChange)

    await user.type(screen.getByLabelText("Path"), "/home/me/dev/new")
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    await user.click(await screen.findByRole("button", { name: "Discard" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(requests.filter((one) => one.method !== "GET")).toEqual([])
  })
})
