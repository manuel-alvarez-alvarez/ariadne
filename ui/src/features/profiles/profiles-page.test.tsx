// @vitest-environment jsdom

/**
 * The one thing on this screen that is not the daemon's: `?expand=<id>`, the
 * link the command palette follows when a profile is picked.
 *
 * Rendered rather than unit-tested because what makes it work is the screen's
 * own state — the role tab is React state that a same-route navigation does not
 * touch, so a pick made while a tab is up has to widen the list back to All or
 * the row that was asked for never mounts. Only the mounted screen shows that.
 *
 * jsdom is asked for by this file alone (the docblock above): every other test
 * in the app is pure and has no business paying for a DOM.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter, useNavigate } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ProfileDto } from "@/api"
import { paths } from "@/routes/paths"

import { ProfilesPage } from "./profiles-page"

/**
 * Hoisted, and not `vi.stubGlobal`: `openapi-fetch` takes its `fetch` when the
 * client is built, which is when `@/api` is imported — a stub installed after
 * that is a stub the daemon client never sees, and the test would go looking
 * for a real daemon.
 */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const ENGINEER: ProfileDto = {
  id: "01JPROF000000000000000ENG",
  name: "Builder",
  role: "engineer",
  agent_kind: "claude_code",
  model: null,
  system_prompt: "",
  extra_flags: [],
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

const REVIEWER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000REV",
  name: "Critic",
  role: "reviewer",
}

/**
 * The daemon's `GET /v1/profiles`, narrowing by role the way it does — and the
 * prompt list an expanded row asks for, which this screen's tests have nothing
 * to say about (`profile-prompts.test.tsx` does) but which has to answer
 * something other than a profile.
 */
function stubDaemon(profiles: ProfileDto[]) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const role = url.searchParams.get("role")
    const body = url.pathname.endsWith("/prompts")
      ? []
      : role
        ? profiles.filter((profile) => profile.role === role)
        : profiles
    return Promise.resolve(
      new Response(JSON.stringify(body), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    )
  })
}

/** The screen, with something next to it that navigates the way the palette does. */
function renderScreen() {
  function PickFromPalette() {
    const navigate = useNavigate()
    return (
      <button type="button" onClick={() => void navigate(paths.profile(ENGINEER.id))}>
        pick Builder
      </button>
    )
  }

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  })
  return render(
    <MemoryRouter initialEntries={[paths.profiles()]}>
      <QueryClientProvider client={queryClient}>
        <PickFromPalette />
        <ProfilesPage />
      </QueryClientProvider>
    </MemoryRouter>,
  )
}

beforeEach(() => {
  // jsdom lays nothing out, so it does not implement this.
  Element.prototype.scrollIntoView = vi.fn()
  daemonFetch.mockReset()
  stubDaemon([ENGINEER, REVIEWER])
})

// Testing Library only unmounts by itself under `globals: true`, which this
// project does not use — without this every screen stays in the document.
afterEach(cleanup)

describe("ProfilesPage, on ?expand=", () => {
  it("expands the profile that was asked for and scrolls to it", async () => {
    const user = userEvent.setup()
    renderScreen()
    await screen.findByRole("button", { name: "Builder" })

    await user.click(screen.getByRole("button", { name: "pick Builder" }))

    const row = await screen.findByRole("button", { name: "Builder", expanded: true })
    expect(row).toBeDefined()
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled()
  })

  it("widens the role tab, so a pick is not filtered out of the list it lands in", async () => {
    const user = userEvent.setup()
    renderScreen()
    await screen.findByRole("button", { name: "Builder" })

    // The reproduction: a tab is up, and the picked profile is not under it.
    await user.click(screen.getByRole("tab", { name: "Reviewer" }))
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Builder" })).toBeNull()
    })

    await user.click(screen.getByRole("button", { name: "pick Builder" }))

    expect(await screen.findByRole("button", { name: "Builder", expanded: true })).toBeDefined()
    expect(screen.getByRole("tab", { name: "All", selected: true })).toBeDefined()
  })

  it("takes the param back off the URL, so closing the row stays closed", async () => {
    const user = userEvent.setup()
    renderScreen()
    await screen.findByRole("button", { name: "Builder" })

    await user.click(screen.getByRole("button", { name: "pick Builder" }))
    const row = await screen.findByRole("button", { name: "Builder", expanded: true })

    await user.click(row)
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Builder" }).getAttribute("aria-expanded")).toBe(
        "false",
      )
    })
  })
})
