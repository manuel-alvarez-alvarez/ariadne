// @vitest-environment jsdom

/**
 * The memory screens against a stubbed daemon: the top-level page over every
 * scope, and one repository's page over its own memories and the global ones.
 * Each lists, filters by scope, searches through the daemon's own search
 * endpoint rather than a client-side filter, adds and deletes — the actions
 * `ariadne memory ls|search|add|delete` offers, per the parity rule of spec 015.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent, { type UserEvent } from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { MemoryDto, RepositoryDto } from "@/api"
import { aMemory, aRepository } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { MemoryPage } from "./memory-page"

const REPOSITORY: RepositoryDto = aRepository({ id: "01JREPO00000000000000ARI" })

const LINT_NOTE: MemoryDto = aMemory({
  id: "01JMEM0000000000000LINT1",
  repository_id: REPOSITORY.id,
  text: "The lint config lives in biome.json, not .eslintrc.",
})

// The other repository row: saved outside a task, which is how an
// orchestrator's own session saves one — its source names a goal and a
// session, but no task.
const MIGRATION_NOTE: MemoryDto = aMemory({
  id: "01JMEM0000000000000MIGR1",
  repository_id: REPOSITORY.id,
  text: "Migrations run from crates/ariadne-store/migrations.",
  source_task_id: null,
})

// A global memory the user saved: no repository, no source, no expiry.
const GLOBAL_NOTE: MemoryDto = aMemory({
  id: "01JMEM00000000000GLOBAL1",
  repository_id: null,
  text: "Write every commit subject as a conventional commit.",
  source_session_id: null,
  source_task_id: null,
  source_goal_id: null,
  expires_at: null,
})

interface Recorded {
  method: string
  url: string
  body: unknown
}

let requests: Recorded[] = []
/** `DELETE /v1/memories/{id}` answers this instead of 204, when set. */
let deleteFailure: { status: number; code: string; message: string } | null = null
/** `POST /v1/memories` answers this instead of 201, when set. */
let createFailure: { status: number; code: string; message: string } | null = null

/** The daemon's own scope rule over `repository` and `scope` (019). */
function inScope(memory: MemoryDto, params: URLSearchParams): boolean {
  const repository = params.get("repository")
  switch (params.get("scope") ?? "all") {
    case "global":
      return memory.repository_id == null
    case "repository":
      return memory.repository_id === repository
    default:
      return !repository || memory.repository_id == null || memory.repository_id === repository
  }
}

function stubDaemon(initial: MemoryDto[]) {
  requests = []
  deleteFailure = null
  createFailure = null
  let memories = initial
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    const body = request.method === "POST" ? await request.json() : undefined
    requests.push({ method: request.method, url: url.pathname + url.search, body })

    if (request.method === "DELETE") {
      if (deleteFailure) {
        const { status, code, message } = deleteFailure
        return errorResponse(status, code, message)
      }
      const id = url.pathname.split("/").at(-1)
      memories = memories.filter((memory) => memory.id !== id)
      return new Response(null, { status: 204 })
    }
    if (request.method === "POST") {
      if (createFailure) {
        const { status, code, message } = createFailure
        return errorResponse(status, code, message)
      }
      const created = aMemory({
        id: `01JMEM0000000000000NEW${memories.length}`,
        source_session_id: null,
        source_task_id: null,
        source_goal_id: null,
        expires_at: null,
        ...(body as Partial<MemoryDto>),
      })
      memories = [created, ...memories]
      return jsonResponse(created, 201)
    }
    if (url.pathname === "/v1/repositories") return jsonResponse([REPOSITORY])
    const scoped = memories.filter((memory) => inScope(memory, url.searchParams))
    if (url.pathname === "/v1/memories/search") {
      // Any query word matches; where none does, the newest of the scope stand in.
      const words = (url.searchParams.get("q") ?? "").toLowerCase().split(/\s+/).filter(Boolean)
      const hits = scoped.filter((memory) =>
        words.some((word) => memory.text.toLowerCase().includes(word)),
      )
      return jsonResponse(
        hits.length ? { hits, fallback: false } : { hits: scoped, fallback: true },
      )
    }
    return jsonResponse(scoped)
  })
}

/** The row a memory is on, found by its own text. */
async function row(text: string): Promise<HTMLElement> {
  const cell = await screen.findByText(text)
  const found = cell.closest("tr")
  if (!found) throw new Error(`no row around "${text}"`)
  return found
}

async function pick(user: UserEvent, combobox: string, option: string) {
  await user.click(screen.getByRole("combobox", { name: combobox }))
  await user.click(await screen.findByRole("option", { name: option }))
}

async function deleteRow(user: UserEvent, memory: MemoryDto) {
  await user.click(screen.getByRole("button", { name: `Delete memory …${memory.id.slice(-8)}` }))
  await user.click(await screen.findByRole("button", { name: "Delete memory" }))
}

const posts = () => requests.filter((call) => call.method === "POST")

beforeEach(() => {
  stubDaemon([LINT_NOTE, MIGRATION_NOTE, GLOBAL_NOTE])
})

describe("the memory page of every scope", () => {
  it("lists memories of both scopes and names the scope of each row", async () => {
    renderScreen(<MemoryPage />)

    expect(within(await row(GLOBAL_NOTE.text)).getByText("Global")).toBeDefined()
    expect(within(await row(LINT_NOTE.text)).getByText("ariadne")).toBeDefined()
    expect(screen.getByText("3 memories")).toBeDefined()

    const list = requests.find((call) => call.url.startsWith("/v1/memories"))
    expect(list?.url).toBe("/v1/memories")
  })

  it("shows the global set alone under the global scope filter", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage />)
    await screen.findByText(LINT_NOTE.text)

    await pick(user, "Filter by scope", "Global")

    await waitFor(() => {
      expect(screen.queryByText(LINT_NOTE.text)).toBeNull()
    })
    expect(screen.getByText(GLOBAL_NOTE.text)).toBeDefined()
    expect(screen.queryByText(MIGRATION_NOTE.text)).toBeNull()
    expect(requests.at(-1)?.url).toBe("/v1/memories?scope=global")
  })

  it("adds a global memory, and the list shows it", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage />)
    await screen.findByText(LINT_NOTE.text)

    await user.click(screen.getByRole("button", { name: "Add memory" }))
    await user.type(await screen.findByLabelText("Text"), "Run the UI checks with npx vitest.")
    await user.click(screen.getByRole("button", { name: "Save memory" }))

    const added = await row("Run the UI checks with npx vitest.")
    expect(within(added).getByText("Global")).toBeDefined()
    expect(posts()).toEqual([
      {
        method: "POST",
        url: "/v1/memories",
        body: { text: "Run the UI checks with npx vitest.", repository_id: null, expires_at: null },
      },
    ])
  })

  it("adds a memory for one repository", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage />)
    await screen.findByText(LINT_NOTE.text)

    await user.click(screen.getByRole("button", { name: "Add memory" }))
    await user.type(await screen.findByLabelText("Text"), "Seed data lives in defaults.rs.")
    await pick(user, "Scope", "ariadne")
    await user.click(screen.getByRole("button", { name: "Save memory" }))

    const added = await row("Seed data lives in defaults.rs.")
    expect(within(added).getByText("ariadne")).toBeDefined()
    expect(posts()[0]?.body).toEqual({
      text: "Seed data lives in defaults.rs.",
      repository_id: REPOSITORY.id,
      expires_at: null,
    })
  })

  it("shows the daemon's refusal and keeps the typed text", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage />)
    await screen.findByText(LINT_NOTE.text)
    createFailure = {
      status: 409,
      code: "duplicate_memory",
      message: "an active memory already holds this text",
    }

    await user.click(screen.getByRole("button", { name: "Add memory" }))
    await user.type(await screen.findByLabelText("Text"), GLOBAL_NOTE.text)
    await user.click(screen.getByRole("button", { name: "Save memory" }))

    expect(await screen.findByText(/an active memory already holds this text/)).toBeDefined()
    expect((screen.getByLabelText("Text") as HTMLTextAreaElement).value).toBe(GLOBAL_NOTE.text)
  })

  it("deletes an entry of either scope", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage />)
    await screen.findByText(LINT_NOTE.text)

    await deleteRow(user, GLOBAL_NOTE)
    await waitFor(() => {
      expect(screen.queryByText(GLOBAL_NOTE.text)).toBeNull()
    })
    await deleteRow(user, LINT_NOTE)
    await waitFor(() => {
      expect(screen.queryByText(LINT_NOTE.text)).toBeNull()
    })

    expect(screen.getByText(MIGRATION_NOTE.text)).toBeDefined()
    expect(requests.filter((call) => call.method === "DELETE").map((call) => call.url)).toEqual([
      `/v1/memories/${GLOBAL_NOTE.id}`,
      `/v1/memories/${LINT_NOTE.id}`,
    ])
  })
})

describe("a repository's memory page", () => {
  it("lists its own memories and the global ones, with text, source and expiry", async () => {
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)

    expect(await screen.findByText(LINT_NOTE.text)).toBeDefined()
    expect(screen.getByText(MIGRATION_NOTE.text)).toBeDefined()
    expect(screen.getByText(GLOBAL_NOTE.text)).toBeDefined()
    expect(screen.getByText("3 memories")).toBeDefined()

    // Expiry: the exact stamp rides on the `<time>` the hint hangs off.
    expect(document.querySelector(`time[datetime="${LINT_NOTE.expires_at}"]`)).not.toBeNull()

    // Source: the session and goal that saved the note, and a dash where the
    // save happened outside a task rather than the empty column that would be.
    const lint = await row(LINT_NOTE.text)
    expect(lint.textContent).toContain("session")
    expect(lint.textContent).toContain("goal")
    const migration = await row(MIGRATION_NOTE.text)
    expect(migration.textContent).toContain("task —")
    const global = await row(GLOBAL_NOTE.text)
    expect(global.textContent).toContain("you")
    expect(global.textContent).toContain("never")

    const list = requests.find((call) => call.url.startsWith("/v1/memories"))
    expect(list?.url).toBe(`/v1/memories?repository=${REPOSITORY.id}`)
  })

  it("narrows to its own memories under its own scope filter", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(GLOBAL_NOTE.text)

    await pick(user, "Filter by scope", "ariadne")

    await waitFor(() => {
      expect(screen.queryByText(GLOBAL_NOTE.text)).toBeNull()
    })
    expect(screen.getByText(LINT_NOTE.text)).toBeDefined()
  })

  it("searches through the daemon's own endpoint rather than filtering locally", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(LINT_NOTE.text)

    await user.type(screen.getByLabelText("Search memory"), "migrations")

    await waitFor(() => {
      expect(screen.queryByText(LINT_NOTE.text)).toBeNull()
    })
    expect(await screen.findByText(MIGRATION_NOTE.text)).toBeDefined()
    expect(screen.queryByText(/No word of/)).toBeNull()

    // Every keystroke fires its own search request; the last one is the query
    // the user actually meant.
    const searchCalls = requests.filter((call) => call.url.startsWith("/v1/memories/search?"))
    expect(searchCalls.at(-1)?.url).toBe(
      `/v1/memories/search?q=migrations&repository=${REPOSITORY.id}`,
    )
  })

  it("adds a memory for its own repository alone", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(LINT_NOTE.text)

    await user.click(screen.getByRole("button", { name: "Add memory" }))
    await user.type(await screen.findByLabelText("Text"), "Seed data lives in defaults.rs.")
    await user.click(screen.getByRole("button", { name: "Save memory" }))

    expect(await screen.findByText("Seed data lives in defaults.rs.")).toBeDefined()
    expect(screen.queryByRole("combobox", { name: "Scope" })).toBeNull()
    expect(posts()[0]?.body).toMatchObject({ repository_id: REPOSITORY.id })
  })

  it("says there is nothing active, before anything is saved", async () => {
    stubDaemon([])
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)

    expect(await screen.findByText("No active memories for this repository.")).toBeDefined()
  })

  it("says no word matched, where the newest memories stand in", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(LINT_NOTE.text)

    await user.type(screen.getByLabelText("Search memory"), "zebra")

    expect(
      await screen.findByText("No word of “zebra” matched. The newest memories stand in."),
    ).toBeDefined()
    expect(screen.getByText(LINT_NOTE.text)).toBeDefined()
  })

  it("removes the row once the daemon confirms the delete", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(LINT_NOTE.text)

    await deleteRow(user, LINT_NOTE)

    await waitFor(() => {
      expect(screen.queryByText(LINT_NOTE.text)).toBeNull()
    })
    expect(screen.getByText(MIGRATION_NOTE.text)).toBeDefined()

    const deleteCall = requests.find((call) => call.method === "DELETE")
    expect(deleteCall?.url).toBe(`/v1/memories/${LINT_NOTE.id}`)
  })

  it("keeps the daemon's delete refusal on screen instead of toasting it away", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(LINT_NOTE.text)
    deleteFailure = { status: 404, code: "not_found", message: "memory not found" }

    await deleteRow(user, LINT_NOTE)

    expect(await screen.findByText("Could not delete the memory")).toBeDefined()
    expect(screen.getByText(LINT_NOTE.text)).toBeDefined()
  })
})
