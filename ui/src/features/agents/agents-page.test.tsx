// @vitest-environment jsdom

/**
 * The agents screen and its dialog against a stubbed daemon.
 *
 * The screen is three rows off a DTO, so what is worth pinning is the round
 * trip: `PUT /v1/agents/{kind}` replaces the flag list *whole*, which means a
 * dropped row has to leave as a shorter list rather than as an empty string,
 * and restoring the defaults has to send the daemon's own `default_flags`
 * back — there is no reset endpoint, and a list hand-copied from anywhere else
 * would silently drift from what Ariadne ships.
 *
 * The rest is what the screen says about a list before it is touched: how many
 * agents there are, whether each list is still the default — the only thing a
 * flag list on its own does not tell a reader — and what a daemon that answers
 * with no agents at all leaves on screen.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { AgentConfigDto } from "@/api"
import { anAgentConfig } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { AgentsPage } from "./agents-page"

/** Customized: the default is there, with one flag added after it. */
const CLAUDE_CODE = anAgentConfig({
  extra_flags: ["--dangerously-skip-permissions", "--verbose"],
  default_flags: ["--dangerously-skip-permissions"],
})

/** Untouched: exactly what Ariadne ships. */
const CODEX = anAgentConfig({
  agent_kind: "codex",
  extra_flags: ["--dangerously-bypass-approvals-and-sandbox"],
  default_flags: ["--dangerously-bypass-approvals-and-sandbox"],
})

/** Emptied: the default dropped, which is a legitimate answer of its own. */
const OPENCODE = anAgentConfig({ agent_kind: "opencode", default_flags: ["--auto"] })

interface Recorded {
  method: string
  path: string
  body: { extra_flags?: string[] } | null
}

let requests: Recorded[] = []

/** The last write that went out, whatever it was. */
function lastWrite(): Recorded | undefined {
  return requests.filter((one) => one.method !== "GET").at(-1)
}

/**
 * The daemon, storing what it is sent.
 *
 * A write is kept rather than only echoed, so the refetch the mutation
 * triggers answers with the new list — which is what makes "the row shows what
 * was saved" a test of the round trip rather than of the optimistic patch.
 */
function stubDaemon(configs: AgentConfigDto[] = [CLAUDE_CODE, CODEX, OPENCODE]) {
  const stored = configs.map((config) => ({ ...config }))
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (request.method === "PUT") {
      const kind = pathname.split("/").at(-1)
      const config = stored.find((one) => one.agent_kind === kind)
      if (!config) return jsonResponse({})
      config.extra_flags = body?.extra_flags ?? []
      return jsonResponse(config)
    }
    return jsonResponse(stored)
  })
}

/** Opens the dialog on one agent, by the row's own edit button. */
async function openFlags(user: ReturnType<typeof userEvent.setup>, label: string) {
  await user.click(await screen.findByRole("button", { name: `Edit ${label} flags` }))
  await screen.findByRole("dialog")
}

beforeEach(() => {
  requests = []
  stubDaemon()
})

// Testing Library only unmounts by itself under `globals: true`, which this
// project does not use — without this every screen stays in the document.

describe("AgentsPage", () => {
  it("lists the three agents by name, with the flags each is launched with", async () => {
    renderScreen(<AgentsPage />)

    expect(await screen.findByText("Claude Code")).toBeDefined()
    expect(screen.getByText("Codex")).toBeDefined()
    expect(screen.getByText("OpenCode")).toBeDefined()

    expect(screen.getByText("--dangerously-skip-permissions")).toBeDefined()
    expect(screen.getByText("--verbose")).toBeDefined()
    expect(screen.getByText("--dangerously-bypass-approvals-and-sandbox")).toBeDefined()
  })

  it("says which lists have been moved off the defaults, in both directions", async () => {
    renderScreen(<AgentsPage />)

    // Claude Code has a flag added, OpenCode has the default dropped; only
    // Codex is still exactly what Ariadne ships.
    expect(await screen.findAllByText("Customized")).toHaveLength(2)
    // The same word the prompt sections use for a text nobody has moved.
    expect(screen.getAllByText("Default")).toHaveLength(1)
    expect(screen.getByText(/none — Ariadne's own arguments only/)).toBeDefined()
  })

  it("counts the rows it is showing", async () => {
    renderScreen(<AgentsPage />)

    expect(await screen.findByText("3 agents")).toBeDefined()
  })

  /**
   * The list is the daemon's, and a daemon that answers with none leaves a
   * headed table over nothing — so the table says it is empty rather than
   * looking like it is still loading.
   */
  it("says the list is empty rather than showing a bare table", async () => {
    stubDaemon([])
    renderScreen(<AgentsPage />)

    expect(await screen.findByText("No agents")).toBeDefined()
    expect(screen.getByText("0 agents")).toBeDefined()
    expect(screen.queryByRole("button", { name: /Edit .* flags/ })).toBeNull()
  })

  it("opens the dialog on the agent of the row it was clicked in", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "Codex")

    expect(screen.getByRole("heading", { name: "Codex flags" })).toBeDefined()
    expect((screen.getByLabelText("Flag 1") as HTMLInputElement).value).toBe(
      "--dangerously-bypass-approvals-and-sandbox",
    )
  })
})

describe("editing an agent's flags", () => {
  it("sends the whole list, added row included, to that kind's endpoint", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "Codex")
    await user.click(screen.getByRole("button", { name: "Add flag" }))
    await user.type(screen.getByLabelText("Flag 2"), "  --search  ")
    await user.click(screen.getByRole("button", { name: "Save flags" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "PUT", path: "/v1/agents/codex" })
    // Trimmed, and the row that was already there kept its place.
    expect(lastWrite()?.body).toEqual({
      extra_flags: ["--dangerously-bypass-approvals-and-sandbox", "--search"],
    })
  })

  it("sends the shorter list when a row is removed, and the empty one when all are", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "Claude Code")
    await user.click(screen.getByRole("button", { name: "Remove flag 2" }))
    await user.click(screen.getByRole("button", { name: "Remove flag 1" }))
    await user.click(screen.getByRole("button", { name: "Save flags" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()?.body).toEqual({ extra_flags: [] })
  })

  it("closes once the write lands, and shows the daemon's answer in the row", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "Claude Code")
    await user.clear(screen.getByLabelText("Flag 2"))
    await user.type(screen.getByLabelText("Flag 2"), "--debug")
    await user.click(screen.getByRole("button", { name: "Save flags" }))

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull()
    })
    expect(await screen.findByText("--debug")).toBeDefined()
  })

  it("keeps a refused write on screen instead of closing on it", async () => {
    const user = userEvent.setup()
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      if (request.method !== "PUT") return jsonResponse([CLAUDE_CODE, CODEX, OPENCODE])
      return new Response(
        JSON.stringify({ error: { code: "bad_request", message: "unknown agent kind: codex" } }),
        { status: 400, headers: { "content-type": "application/json" } },
      )
    })
    renderScreen(<AgentsPage />)

    await openFlags(user, "Codex")
    await user.click(screen.getByRole("button", { name: "Save flags" }))

    const alert = await screen.findByRole("alert")
    expect(alert.textContent).toContain("unknown agent kind")
    expect(screen.getByRole("dialog")).toBeDefined()
  })
})

describe("restoring the defaults", () => {
  it("fills the rows with the daemon's own default_flags and sends those back", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    // OpenCode's default was dropped, so it has no rows to start from.
    await openFlags(user, "OpenCode")
    await user.click(screen.getByRole("button", { name: "Restore defaults" }))

    expect((screen.getByLabelText("Flag 1") as HTMLInputElement).value).toBe("--auto")

    await user.click(screen.getByRole("button", { name: "Save flags" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "PUT", path: "/v1/agents/opencode" })
    expect(lastWrite()?.body).toEqual({ extra_flags: ["--auto"] })
  })

  it("writes nothing on its own — the restore is a form edit like any other", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "Claude Code")
    await user.click(screen.getByRole("button", { name: "Restore defaults" }))

    expect(lastWrite()).toBeUndefined()
    expect(screen.getByRole("dialog")).toBeDefined()
  })

  it("is not offered on a list that is already the default", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "Codex")

    expect(screen.queryByRole("button", { name: "Restore defaults" })).toBeNull()
  })
})
