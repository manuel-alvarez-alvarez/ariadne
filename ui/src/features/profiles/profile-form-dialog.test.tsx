// @vitest-environment jsdom

/**
 * The model combobox inside the profile dialog, against a stubbed daemon.
 *
 * Rendered rather than unit-tested because what matters is the interplay: the
 * catalog list is a convenience bolted onto a free-text field, and every test
 * here is one of the ways that must not curdle — a pick must land in the field,
 * typed text must survive to the request untouched, clearing must fall back to
 * the daemon's `default` sentinel, and a dead catalog endpoint must leave the
 * field a plain input rather than a broken combobox.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ModelDto, ProfileDto } from "@/api"

import { ProfileFormDialog } from "./profile-form-dialog"

/** Hoisted for the reason `profiles-page.test.tsx` gives: the client takes its
 * `fetch` when `@/api` is imported, so a later stub is one it never sees. */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const STAMP = "2026-01-01T00:00:00Z"

const PROFILE: ProfileDto = {
  id: "01JPROF000000000000000ENG",
  name: "Builder",
  role: "engineer",
  agent_kind: "claude_code",
  model: "claude-opus-5",
  system_prompt: "Stored system prompt.",
  extra_flags: [],
  created_at: STAMP,
  updated_at: STAMP,
}

/** A slice of the daemon's curated catalog, two agents wide. */
const CATALOG: ModelDto[] = [
  { id: "claude-fable-5", agent_kind: "claude_code", description: "Frontier: highest capability" },
  { id: "claude-opus-5", agent_kind: "claude_code", description: "Opus tier: deep analysis" },
  { id: "gpt-5.5-codex", agent_kind: "codex", description: "Frontier reasoning: agentic loops" },
]

interface Recorded {
  method: string
  path: string
  body: { model?: string | null } | null
}

let requests: Recorded[] = []

/** The last request that went to `path` with `method`, or undefined. */
function lastRequest(method: string, path: string): Recorded | undefined {
  return requests.filter((one) => one.method === method && one.path === path).at(-1)
}

/** The models endpoint plus the two profile writes, echoing like the daemon. */
function stubDaemon({ models = CATALOG }: { models?: ModelDto[] | "error" } = {}) {
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

    if (pathname === "/v1/models") {
      return models === "error" ? answer({ error: "boom" }, 500) : answer(models)
    }
    if (pathname === "/v1/profiles" && request.method === "POST") {
      return answer({ ...PROFILE, ...body, id: "01JPROF00000000000000NEW" })
    }
    if (pathname === `/v1/profiles/${PROFILE.id}` && request.method === "PUT") {
      return answer({ ...PROFILE, ...body })
    }
    return new Response("not stubbed", { status: 404 })
  })
}

function renderDialog(profile: ProfileDto | null) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  return render(
    <QueryClientProvider client={queryClient}>
      <ProfileFormDialog open onOpenChange={() => {}} profile={profile} />
    </QueryClientProvider>,
  )
}

/** The combobox input, once the dialog is up. */
async function modelBox(): Promise<HTMLInputElement> {
  return (await screen.findByRole("combobox", { name: "Model" })) as HTMLInputElement
}

/** The catalog popup, which lives in a portal outside the dialog. */
async function listbox(): Promise<HTMLElement> {
  return await screen.findByRole("listbox", { name: "Models" })
}

beforeEach(() => {
  // jsdom lays nothing out, so it implements neither of these.
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

describe("the model combobox", () => {
  it("lists the catalog with descriptions on click, and a pick fills the field", async () => {
    const user = userEvent.setup()
    renderDialog(null)
    const box = await modelBox()

    await user.click(box)
    const options = await listbox()

    // With Auto-resolve selected the union is grouped, one heading per agent.
    expect(within(options).getByText("Claude Code")).toBeDefined()
    expect(within(options).getByText("Codex")).toBeDefined()
    expect(within(options).getByText("Opus tier: deep analysis")).toBeDefined()

    await user.click(within(options).getByText("claude-opus-5"))

    expect(box.value).toBe("claude-opus-5")
    await waitFor(() => {
      expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
    })
  })

  it("filters while typing and picks with the keyboard", async () => {
    const user = userEvent.setup()
    renderDialog(null)
    const box = await modelBox()

    await user.type(box, "codex")
    const options = await listbox()
    await waitFor(() => {
      expect(within(options).queryByText("claude-fable-5")).toBeNull()
    })
    expect(within(options).getByText("gpt-5.5-codex")).toBeDefined()

    await user.keyboard("{ArrowDown}{Enter}")

    expect(box.value).toBe("gpt-5.5-codex")
  })

  it("re-scopes the options to the agent choice without touching the value", async () => {
    const user = userEvent.setup()
    renderDialog(null)
    const box = await modelBox()
    await user.type(box, "kept-as-typed")

    await user.click(screen.getByRole("combobox", { name: "Agent" }))
    await user.click(await screen.findByRole("option", { name: "Codex" }))

    expect(box.value).toBe("kept-as-typed")

    await user.click(box)
    const options = await listbox()
    // The filter still applies ("kept-as-typed" matches nothing), so widen it.
    await user.clear(box)
    expect(within(options).getByText("gpt-5.5-codex")).toBeDefined()
    expect(within(options).queryByText("claude-opus-5")).toBeNull()
    expect(within(options).queryByText("Codex")).toBeNull() // no heading when pinned
  })

  it("sends typed free text as the model, matched by nothing in the catalog", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Name"), "custom")
    await user.type(await modelBox(), "my-weird-model")
    await user.type(screen.getByLabelText("System prompt"), "Do things.")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => {
      expect(lastRequest("POST", "/v1/profiles")).toBeDefined()
    })
    expect(lastRequest("POST", "/v1/profiles")?.body?.model).toBe("my-weird-model")
  })

  it("clears back to the provider default, which the update spells as its sentinel", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)
    const box = await modelBox()
    expect(box.value).toBe("claude-opus-5")

    await user.click(screen.getByRole("button", { name: "Use default" }))
    expect(box.value).toBe("")

    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastRequest("PUT", `/v1/profiles/${PROFILE.id}`)).toBeDefined()
    })
    expect(lastRequest("PUT", `/v1/profiles/${PROFILE.id}`)?.body?.model).toBe("default")
  })

  it("degrades to a plain free-text field when the catalog cannot be fetched", async () => {
    const user = userEvent.setup()
    stubDaemon({ models: "error" })
    renderDialog(null)
    const box = await modelBox()

    // No catalog: clicking and typing open nothing, and saving still works.
    await user.click(box)
    await user.type(box, "still-typable")
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
    expect(box.value).toBe("still-typable")

    await user.type(screen.getByLabelText("Name"), "offline")
    await user.type(screen.getByLabelText("System prompt"), "Do things.")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => {
      expect(lastRequest("POST", "/v1/profiles")).toBeDefined()
    })
    expect(lastRequest("POST", "/v1/profiles")?.body?.model).toBe("still-typable")
  })
})
