// @vitest-environment jsdom

/**
 * The details panel's prompts, against a stubbed daemon.
 *
 * Rendered rather than unit-tested because everything worth checking is a round
 * trip: which prompts a role gets is the daemon's answer and not a map in the
 * UI, and the panel has to show exactly those and nothing of the other roles'.
 *
 * The panel is read-only — prompts are edited in the profile dialog, which
 * `profile-form-dialog.test.tsx` covers — so the other thing checked here is
 * that it offers nothing to type into, save or restore.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ProfileDto, ProfilePromptDto } from "@/api"

import { ProfilePrompts } from "./profile-prompts"

/** Hoisted for the reason `profiles-page.test.tsx` gives: the client takes its
 * `fetch` when `@/api` is imported, so a later stub is one it never sees. */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const STAMP = "2026-01-01T00:00:00Z"

const ENGINEER: ProfileDto = {
  id: "01JPROF000000000000000ENG",
  name: "Builder",
  role: "engineer",
  agent_kind: "claude_code",
  model: null,
  system_prompt: "Stored system prompt.",
  system_prompt_is_default: false,
  created_at: STAMP,
  updated_at: STAMP,
}

const PLANNER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000PLN",
  name: "Mapper",
  role: "planner",
}

const REVIEWER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000REV",
  name: "Critic",
  role: "reviewer",
}

/** What the daemon holds, per role — the kinds it answers `GET .../prompts` with. */
const STORED: Record<string, ProfilePromptDto[]> = {
  engineer: [
    {
      kind: "engineer_briefing",
      content: "Stored engineer briefing.",
      is_default: false,
      updated_at: STAMP,
    },
    {
      kind: "engineer_resume",
      content: "Stored engineer resume.",
      is_default: false,
      updated_at: STAMP,
    },
    {
      kind: "changes_requested",
      content: "Stored changes requested.",
      is_default: false,
      updated_at: STAMP,
    },
    {
      kind: "landing_direct",
      content: "Stored direct landing.",
      is_default: false,
      updated_at: STAMP,
    },
    {
      kind: "landing_pull_request",
      content: "Stored published landing.",
      is_default: false,
      updated_at: STAMP,
    },
    {
      kind: "message_delivery",
      content: "Stored message delivery.",
      is_default: false,
      updated_at: STAMP,
    },
  ],
  planner: [
    {
      kind: "planner_briefing",
      content: "Stored planner briefing.",
      is_default: false,
      updated_at: STAMP,
    },
  ],
  reviewer: [
    {
      kind: "reviewer_briefing",
      content: "Stored reviewer briefing.",
      is_default: false,
      updated_at: STAMP,
    },
    {
      kind: "reviewer_resume",
      content: "Stored reviewer resume.",
      is_default: false,
      updated_at: STAMP,
    },
  ],
}

interface Recorded {
  method: string
  path: string
}

let requests: Recorded[] = []

/** `GET /v1/profiles/{id}/prompts`, and nothing else this panel could need. */
function stubDaemon(profile: ProfileDto) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    requests.push({ method: request.method, path: pathname })

    if (pathname === `/v1/profiles/${profile.id}/prompts`) {
      return new Response(JSON.stringify(STORED[profile.role] ?? []), {
        status: 200,
        headers: { "content-type": "application/json" },
      })
    }

    return new Response("not stubbed", { status: 404 })
  })
}

function renderPrompts(profile: ProfileDto) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  return render(
    <QueryClientProvider client={queryClient}>
      <ProfilePrompts profile={profile} />
    </QueryClientProvider>,
  )
}

/** The block one prompt is shown in, once its content has arrived. */
async function shown(label: string): Promise<HTMLElement> {
  return await screen.findByLabelText(label)
}

beforeEach(() => {
  requests = []
  daemonFetch.mockReset()
  stubDaemon(ENGINEER)
})

afterEach(cleanup)

describe("ProfilePrompts", () => {
  it("shows the system prompt and every briefing the daemon lists for the role", async () => {
    renderPrompts(ENGINEER)

    expect((await shown("System prompt")).textContent).toBe("Stored system prompt.")
    expect((await shown("Engineer briefing")).textContent).toBe("Stored engineer briefing.")
    expect((await shown("Engineer resume")).textContent).toBe("Stored engineer resume.")
    expect((await shown("Changes requested")).textContent).toBe("Stored changes requested.")
    // The notice every role is woken with for a message addressed to it.
    expect((await shown("Message delivery")).textContent).toBe("Stored message delivery.")
    // One landing procedure per merge strategy, both the engineer's own.
    expect((await shown("Landing (direct)")).textContent).toBe("Stored direct landing.")
    expect((await shown("Landing (pull request)")).textContent).toBe("Stored published landing.")
    // A kind of another role is a kind the daemon never sent.
    expect(screen.queryByLabelText("Reviewer briefing")).toBeNull()
  })

  it("shows a planner its one briefing and none of the engineer's", async () => {
    stubDaemon(PLANNER)
    renderPrompts(PLANNER)

    expect((await shown("Planner briefing")).textContent).toBe("Stored planner briefing.")
    expect(screen.queryByLabelText("Engineer briefing")).toBeNull()
    expect(screen.queryByLabelText("Changes requested")).toBeNull()
  })

  it("shows a reviewer both of its briefings and none of the other roles'", async () => {
    stubDaemon(REVIEWER)
    renderPrompts(REVIEWER)

    expect((await shown("System prompt")).textContent).toBe("Stored system prompt.")
    expect((await shown("Reviewer briefing")).textContent).toBe("Stored reviewer briefing.")
    expect((await shown("Reviewer resume")).textContent).toBe("Stored reviewer resume.")
    expect(screen.queryByLabelText("Planner briefing")).toBeNull()
    expect(screen.queryByLabelText("Engineer briefing")).toBeNull()
    expect(screen.queryByLabelText("Changes requested")).toBeNull()
    expect(screen.queryByLabelText("Landing instructions")).toBeNull()
  })

  it("names each prompt and says when the daemon sends it", async () => {
    renderPrompts(ENGINEER)
    await shown("Engineer briefing")

    // The label is a line of its own, not the block's `aria-label` alone.
    expect(screen.getByText("Engineer briefing")).toBeDefined()
    expect(screen.getByText("Starts the engineer on a task.")).toBeDefined()
  })

  it("offers no way to edit a prompt: the dialog is the only one", async () => {
    renderPrompts(ENGINEER)
    await shown("Engineer briefing")

    expect(screen.queryAllByRole("textbox")).toEqual([])
    expect(screen.queryAllByRole("button")).toEqual([])
  })

  it("writes nothing to the daemon: reading the prompts is all it does", async () => {
    renderPrompts(ENGINEER)
    await shown("Changes requested")

    expect(requests.every((one) => one.method === "GET")).toBe(true)
  })

  it("says so when the prompts cannot be loaded", async () => {
    daemonFetch.mockImplementation(async () => new Response("boom", { status: 500 }))
    renderPrompts(ENGINEER)

    expect(await screen.findByText("Could not load the prompts")).toBeDefined()
    // The system prompt comes off the profile itself, so it is there regardless.
    expect((await shown("System prompt")).textContent).toBe("Stored system prompt.")
  })
})
