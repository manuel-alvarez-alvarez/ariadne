// @vitest-environment jsdom

/**
 * The memory screen against a stubbed daemon: one repository's saved entries,
 * a search box backed by the daemon's own search endpoint rather than a
 * client-side filter, and delete — the three actions `ariadne memory
 * ls|search|delete` offers, per the parity rule of spec 015.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
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

// The other row: saved outside a task, which is how an orchestrator's own
// session saves one — its source names a goal and a session, but no task.
const MIGRATION_NOTE: MemoryDto = aMemory({
  id: "01JMEM0000000000000MIGR1",
  repository_id: REPOSITORY.id,
  text: "Migrations run from crates/ariadne-store/migrations.",
  source_task_id: null,
})

interface Recorded {
  method: string
  url: string
}

let requests: Recorded[] = []
/** `DELETE .../memories/{id}` answers this instead of 204, when set. */
let deleteFailure: { status: number; code: string; message: string } | null = null

function stubDaemon(initial: MemoryDto[]) {
  requests = []
  deleteFailure = null
  let memories = initial
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    requests.push({ method: request.method, url: url.pathname + url.search })

    if (request.method === "DELETE") {
      if (deleteFailure) {
        const { status, code, message } = deleteFailure
        return errorResponse(status, code, message)
      }
      const id = url.pathname.split("/").at(-1)
      memories = memories.filter((memory) => memory.id !== id)
      return new Response(null, { status: 204 })
    }
    if (url.pathname.endsWith("/search")) {
      const q = url.searchParams.get("q")?.toLowerCase() ?? ""
      return jsonResponse(memories.filter((memory) => memory.text.toLowerCase().includes(q)))
    }
    if (url.pathname === "/v1/repositories") return jsonResponse([REPOSITORY])
    return jsonResponse(memories)
  })
}

/** The row a memory is on, found by its own text. */
async function row(text: string): Promise<HTMLElement> {
  const cell = await screen.findByText(text)
  const found = cell.closest("tr")
  if (!found) throw new Error(`no row around "${text}"`)
  return found
}

beforeEach(() => {
  stubDaemon([LINT_NOTE, MIGRATION_NOTE])
})

describe("MemoryPage", () => {
  it("lists what the daemon holds, with text, source and expiry", async () => {
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)

    expect(await screen.findByText(LINT_NOTE.text)).toBeDefined()
    expect(screen.getByText(MIGRATION_NOTE.text)).toBeDefined()
    expect(screen.getByText("2 memories")).toBeDefined()

    // Expiry: the exact stamp rides on the `<time>` the hint hangs off.
    expect(document.querySelector(`time[datetime="${LINT_NOTE.expires_at}"]`)).not.toBeNull()

    // Source: the session and goal that saved the note, and a dash where the
    // save happened outside a task rather than the empty column that would be.
    const lint = await row(LINT_NOTE.text)
    expect(lint.textContent).toContain("session")
    expect(lint.textContent).toContain("goal")
    const migration = await row(MIGRATION_NOTE.text)
    expect(migration.textContent).toContain("task —")
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

    // Every keystroke fires its own search request; the last one is the query
    // the user actually meant.
    const searchCalls = requests.filter((call) => call.url.includes("/search?q="))
    expect(searchCalls.at(-1)?.url).toContain("q=migrations")
  })

  it("says there is nothing active, before anything is saved", async () => {
    stubDaemon([])
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)

    expect(await screen.findByText("No active memories for this repository.")).toBeDefined()
  })

  it("says nothing matched, once a search comes back empty", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(LINT_NOTE.text)

    await user.type(screen.getByLabelText("Search memory"), "nothing like this")

    expect(await screen.findByText("No memory matches “nothing like this”.")).toBeDefined()
  })
})

describe("deleting a memory", () => {
  it("removes the row once the daemon confirms it", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(LINT_NOTE.text)

    await user.click(
      screen.getByRole("button", { name: `Delete memory …${LINT_NOTE.id.slice(-8)}` }),
    )
    await user.click(await screen.findByRole("button", { name: "Delete memory" }))

    await waitFor(() => {
      expect(screen.queryByText(LINT_NOTE.text)).toBeNull()
    })
    expect(screen.getByText(MIGRATION_NOTE.text)).toBeDefined()

    const deleteCall = requests.find((call) => call.method === "DELETE")
    expect(deleteCall?.url).toBe(`/v1/repositories/${REPOSITORY.id}/memories/${LINT_NOTE.id}`)
  })

  it("keeps the daemon's refusal on screen instead of toasting it away", async () => {
    const user = userEvent.setup()
    renderScreen(<MemoryPage repositoryId={REPOSITORY.id} />)
    await screen.findByText(LINT_NOTE.text)
    deleteFailure = { status: 404, code: "not_found", message: "memory not found" }

    await user.click(
      screen.getByRole("button", { name: `Delete memory …${LINT_NOTE.id.slice(-8)}` }),
    )
    await user.click(await screen.findByRole("button", { name: "Delete memory" }))

    expect(await screen.findByText("Could not delete the memory")).toBeDefined()
    expect(screen.getByText(LINT_NOTE.text)).toBeDefined()
  })
})
