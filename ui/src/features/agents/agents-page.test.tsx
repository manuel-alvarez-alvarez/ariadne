// @vitest-environment jsdom

/**
 * The agents screen and its dialog against a stubbed daemon.
 *
 * The screen is three rows off a DTO, so what is worth pinning is the round
 * trip: `PUT /v1/agents/{id}` replaces the flag list *whole*, which means a
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

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { AgentConfigDto, ModelDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { aModel, anAgentConfig } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { AgentsPage } from "./agents-page"

/** Customized: the default is there, with one flag added after it. */
const CLAUDE_CODE = anAgentConfig({
  extra_flags: ["--dangerously-skip-permissions", "--verbose"],
  default_flags: ["--dangerously-skip-permissions"],
})

/** Untouched: exactly what Ariadne ships. */
const CODEX = anAgentConfig({
  agent_id: "codex-acp",
  extra_flags: ["--dangerously-bypass-approvals-and-sandbox"],
  default_flags: ["--dangerously-bypass-approvals-and-sandbox"],
})

/** Emptied: the default dropped, which is a legitimate answer of its own. */
const OPENCODE = anAgentConfig({ agent_id: "opencode-acp", default_flags: ["--auto"] })

const OPUS = aModel({
  id: "claude-code-acp:claude-opus-5",
  description: "the frontier model",
  tier: "frontier",
})
/** Offered by opencode-acp, so its id carries a `/` of its own. */
const LOCAL = aModel({
  id: "opencode-acp:anthropic/claude-sonnet-4",
  agent_id: "opencode-acp",
  enabled: false,
})

interface Recorded {
  method: string
  path: string
  body: { extra_flags?: string[]; id?: string; enabled?: boolean } | null
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
function stubDaemon(
  configs: AgentConfigDto[] = [CLAUDE_CODE, CODEX, OPENCODE],
  models: ModelDto[] = [OPUS, LOCAL],
  /** Stands in for the one refusal there is: the last model left on. */
  refuse?: string,
) {
  const stored = configs.map((config) => ({ ...config }))
  const catalog = models.map((model) => ({ ...model }))
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (pathname === "/v1/models/enabled") {
      if (refuse !== undefined) return errorResponse(409, "conflict", refuse)
      const model = catalog.find((one) => one.id === body?.id)
      if (!model) return jsonResponse({})
      model.enabled = body?.enabled ?? true
      return jsonResponse(model)
    }
    if (pathname === "/v1/models") return jsonResponse(catalog)

    if (request.method === "PUT") {
      const kind = pathname.split("/").at(-1)
      const config = stored.find((one) => one.agent_id === kind)
      if (!config) return jsonResponse({})
      config.extra_flags = body?.extra_flags ?? []
      return jsonResponse(config)
    }
    return jsonResponse(stored)
  })
}

/** The switch on one model's row, by the accessible name it carries. */
function toggle(id: string): Promise<HTMLElement> {
  return screen.findByRole("switch", { name: `${id} available` })
}

/**
 * Brings one agent's tab to the front. Only the open tab is rendered, so every
 * assertion about an agent other than the first has to come through here.
 */
async function selectAgent(user: ReturnType<typeof userEvent.setup>, label: string) {
  await user.click(await screen.findByRole("tab", { name: new RegExp(`^${label}`) }))
  return screen.findByRole("tabpanel")
}

/** Opens the dialog on one agent, by its own edit button. */
async function openFlags(user: ReturnType<typeof userEvent.setup>, label: string) {
  await selectAgent(user, label)
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
  it("gives every agent a tab, and opens on the daemon's first", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    // The strip is the list of agents there are, whichever one is open.
    expect(await screen.findByRole("tab", { name: /^claude-code-acp/ })).toBeDefined()
    expect(screen.getByRole("tab", { name: /^codex-acp/ })).toBeDefined()
    expect(screen.getByRole("tab", { name: /^opencode-acp/ })).toBeDefined()

    // The first is open, so its flags are the ones on screen.
    expect(screen.getByText("--dangerously-skip-permissions")).toBeDefined()
    expect(screen.getByText("--verbose")).toBeDefined()
    expect(screen.queryByText("--dangerously-bypass-approvals-and-sandbox")).toBeNull()

    await selectAgent(user, "codex-acp")
    expect(await screen.findByText("--dangerously-bypass-approvals-and-sandbox")).toBeDefined()
    expect(screen.queryByText("--dangerously-skip-permissions")).toBeNull()
  })

  it("says whether the open tab's list has been moved off the defaults", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    // claude-code-acp has a flag added, opencode-acp has the default dropped; only
    // codex-acp is still exactly what Ariadne ships.
    expect(await screen.findByText("Customized")).toBeDefined()

    await selectAgent(user, "codex-acp")
    // The same word the prompt sections use for a text nobody has moved.
    expect(await screen.findByText("Default")).toBeDefined()

    await selectAgent(user, "opencode-acp")
    expect(await screen.findByText("Customized")).toBeDefined()
    expect(screen.getByText(/none — Ariadne's own arguments only/)).toBeDefined()
  })

  it("counts both what it lists and what those can be staffed on", async () => {
    renderScreen(<AgentsPage />)

    // One line for the page, because the catalog is now part of it: how many
    // agents there are, how many models between them, and — the fact no single
    // row carries — how many of those are turned off.
    expect(await screen.findByText("3 agents, 2 models, 1 turned off")).toBeDefined()
  })

  it("shows only concrete model ids in the agent tables", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    const claude = await screen.findByRole("tabpanel")
    expect(await within(claude).findByText("claude-code-acp:claude-opus-5")).toBeDefined()
    expect(within(claude).queryByText(/^claude-code-acp$/)).toBeNull()
    const opencode = await selectAgent(user, "opencode-acp")
    expect(
      await within(opencode).findByText("opencode-acp:anthropic/claude-sonnet-4"),
    ).toBeDefined()
    expect(within(opencode).queryByText(/^opencode-acp$/)).toBeNull()
  })

  /**
   * The list is the daemon's, and a daemon that answers with none leaves a
   * headed table over nothing — so the table says it is empty rather than
   * looking like it is still loading.
   */
  it("says the list is empty rather than showing a bare table", async () => {
    stubDaemon([], [])
    renderScreen(<AgentsPage />)

    expect(await screen.findByText("No agents")).toBeDefined()
    expect(screen.getByText("0 agents, 0 models")).toBeDefined()
    expect(screen.queryByRole("button", { name: /Edit .* flags/ })).toBeNull()
  })

  it("opens the dialog on the agent of the row it was clicked in", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "codex-acp")

    expect(screen.getByRole("heading", { name: "codex-acp flags" })).toBeDefined()
    expect((screen.getByLabelText("Flag 1") as HTMLInputElement).value).toBe(
      "--dangerously-bypass-approvals-and-sandbox",
    )
  })
})

describe("editing an agent's flags", () => {
  it("sends the whole list, added row included, to that agent's endpoint", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "codex-acp")
    await user.click(screen.getByRole("button", { name: "Add flag" }))
    await user.type(screen.getByLabelText("Flag 2"), "  --search  ")
    await user.click(screen.getByRole("button", { name: "Save flags" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "PUT", path: "/v1/agents/codex-acp" })
    // Trimmed, and the row that was already there kept its place.
    expect(lastWrite()?.body).toEqual({
      extra_flags: ["--dangerously-bypass-approvals-and-sandbox", "--search"],
    })
  })

  it("sends the shorter list when a row is removed, and the empty one when all are", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "claude-code-acp")
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

    await openFlags(user, "claude-code-acp")
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
      // The catalog is a separate request and not what this is about; answering
      // it with the agent list would hand the models table agent configs.
      if (new URL(request.url).pathname === "/v1/models") return jsonResponse([])
      if (request.method !== "PUT") return jsonResponse([CLAUDE_CODE, CODEX, OPENCODE])
      return new Response(
        JSON.stringify({ error: { code: "bad_request", message: "unknown agent: codex-acp" } }),
        { status: 400, headers: { "content-type": "application/json" } },
      )
    })
    renderScreen(<AgentsPage />)

    await openFlags(user, "codex-acp")
    await user.click(screen.getByRole("button", { name: "Save flags" }))

    const alert = await screen.findByRole("alert")
    expect(alert.textContent).toContain("unknown agent")
    expect(screen.getByRole("dialog")).toBeDefined()
  })
})

describe("restoring the defaults", () => {
  it("fills the rows with the daemon's own default_flags and sends those back", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    // opencode-acp's default was dropped, so it has no rows to start from.
    await openFlags(user, "opencode-acp")
    await user.click(screen.getByRole("button", { name: "Restore defaults" }))

    expect((screen.getByLabelText("Flag 1") as HTMLInputElement).value).toBe("--auto")

    await user.click(screen.getByRole("button", { name: "Save flags" }))

    await waitFor(() => {
      expect(lastWrite()).toBeDefined()
    })
    expect(lastWrite()).toMatchObject({ method: "PUT", path: "/v1/agents/opencode-acp" })
    expect(lastWrite()?.body).toEqual({ extra_flags: ["--auto"] })
  })

  it("writes nothing on its own — the restore is a form edit like any other", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "claude-code-acp")
    await user.click(screen.getByRole("button", { name: "Restore defaults" }))

    expect(lastWrite()).toBeUndefined()
    expect(screen.getByRole("dialog")).toBeDefined()
  })

  it("is not offered on a list that is already the default", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await openFlags(user, "codex-acp")

    expect(screen.queryByRole("button", { name: "Restore defaults" })).toBeNull()
  })
})

/**
 * The catalog, now that it is part of this screen rather than one of its own.
 *
 * The rows sit under the agent that runs them, so what is worth pinning beyond
 * the round trip is that they land in the right section — and that the id
 * still travels in the *body*, because a model id carries both `:` and, for
 * the ids some agents offer, `/`, which no path segment holds.
 */
describe("the models under each agent", () => {
  it("puts each model in the tab of the agent that runs it", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    // claude-code-acp's tab holds the Claude model and not opencode-acp's.
    const claude = (await screen.findByRole("tabpanel")) as HTMLElement
    expect(within(claude).getByText(OPUS.id)).toBeDefined()
    expect(within(claude).getByText("the frontier model")).toBeDefined()
    expect(within(claude).queryByText(LOCAL.id)).toBeNull()

    const opencode = (await selectAgent(user, "opencode-acp")) as HTMLElement
    expect(within(opencode).getByText(LOCAL.id)).toBeDefined()
    expect(within(opencode).queryByText(OPUS.id)).toBeNull()
  })

  /** Each tab carries the size of the catalog behind it, loaded or not. */
  it("counts each agent's catalog on its tab", async () => {
    renderScreen(<AgentsPage />)

    // The pill's own `aria-label` runs straight on after the agent's name, which
    // is how every tab strip in the app already reads.
    expect(await screen.findByRole("tab", { name: /^claude-code-acp\s*1 model$/ })).toBeDefined()
    expect(screen.getByRole("tab", { name: /^opencode-acp\s*1 model$/ })).toBeDefined()
    // An agent the catalog has nothing for still says so, rather than nothing.
    expect(screen.getByRole("tab", { name: /^codex-acp\s*0 models$/ })).toBeDefined()
  })

  it("shows whether each one can be staffed on", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    expect((await toggle(OPUS.id)).getAttribute("aria-checked")).toBe("true")
    await selectAgent(user, "opencode-acp")
    expect((await toggle(LOCAL.id)).getAttribute("aria-checked")).toBe("false")
  })

  it("sends the id in the body, so a model named with a slash still travels", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await selectAgent(user, "opencode-acp")
    await user.click(await toggle(LOCAL.id))

    await waitFor(() => expect(lastWrite()).toBeDefined())
    const write = lastWrite()
    expect(write?.method).toBe("PUT")
    // Not `/v1/models/opencode-acp:anthropic/claude-sonnet-4`, which is two
    // segments and matches no route.
    expect(write?.path).toBe("/v1/models/enabled")
    expect(write?.body).toEqual({ id: LOCAL.id, enabled: true })

    await waitFor(async () =>
      expect((await toggle(LOCAL.id)).getAttribute("aria-checked")).toBe("true"),
    )
  })

  it("turns a model off", async () => {
    const user = userEvent.setup()
    renderScreen(<AgentsPage />)

    await user.click(await toggle(OPUS.id))

    await waitFor(() => expect(lastWrite()?.body).toEqual({ id: OPUS.id, enabled: false }))
    await waitFor(async () =>
      expect((await toggle(OPUS.id)).getAttribute("aria-checked")).toBe("false"),
    )
  })

  /**
   * The daemon refuses the last model left on. A switch that springs back with
   * nothing said reads as a click that never registered, so the reason it gave
   * is what the screen shows — and the row goes back to what the daemon still
   * says rather than to what was asked for.
   */
  it("says why, where the daemon refuses to turn a model off", async () => {
    const refusal = "`claude-code-acp:claude-opus-5` is the last model left enabled"
    stubDaemon([CLAUDE_CODE], [{ ...OPUS, enabled: true }], refusal)
    const user = userEvent.setup()
    renderScreen(
      <>
        <Toaster />
        <AgentsPage />
      </>,
    )

    await user.click(await toggle(OPUS.id))

    expect(await screen.findByText(refusal)).toBeDefined()
    expect((await toggle(OPUS.id)).getAttribute("aria-checked")).toBe("true")
  })

  /**
   * An agent the daemon knows but reported no catalog for still gets its section:
   * the flags are the other half of it, and a headed table over nothing looks
   * like one that is still loading.
   */
  it("says which agent has no catalog rather than leaving a bare table", async () => {
    stubDaemon([CLAUDE_CODE], [])
    renderScreen(<AgentsPage />)

    expect(await screen.findByText(/the daemon reported none for this agent/)).toBeDefined()
    expect(screen.getByText("ariadne models ls")).toBeDefined()
    // The flags are still there: only the catalog was empty.
    expect(screen.getByText("--verbose")).toBeDefined()
  })
})
