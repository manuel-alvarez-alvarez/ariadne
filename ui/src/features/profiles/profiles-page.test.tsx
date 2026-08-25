// @vitest-environment jsdom

/**
 * The one thing on this screen that is not the daemon's: its view state, which
 * lives in the URL — `?expand=<id>`, the link the command palette follows when
 * a profile is picked, and `?role=`, the tab strip over the table.
 *
 * Rendered rather than unit-tested because what is being checked is the round
 * trip: a navigation goes in, and what the table shows comes out. That covers
 * the two rules the params carry — a pick lands on an unfiltered list, since
 * a link has an id and no role; and an expansion is a history step, so Back
 * closes the row it opened — neither of which is visible from the params alone.
 *
 * jsdom is asked for by this file alone (the docblock above): every other test
 * in the app is pure and has no business paying for a DOM.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter, useLocation, useNavigate } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ModelDto, ProfileDto } from "@/api"
import { PROFILE_EXPAND_PARAM, paths } from "@/routes/paths"

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
  system_prompt_is_default: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

const REVIEWER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000REV",
  name: "Critic",
  role: "reviewer",
}

/** A profile pinned to a model the catalog below knows about. */
const PINNED: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000PIN",
  name: "Pinned",
  model: "claude-opus-5",
}

/** The model catalog an expanded row asks for, to caption a known model with. */
const CATALOG: ModelDto[] = [
  { id: "claude-opus-5", agent_kind: "claude_code", description: "Opus tier: deep analysis" },
]

/**
 * The daemon's `GET /v1/profiles`, narrowing by role the way it does — plus
 * the model catalog and the prompt list an expanded row asks for. The prompts
 * are not this screen's tests' business (`profile-prompts.test.tsx`), but they
 * have to answer something other than a profile.
 */
function stubDaemon(profiles: ProfileDto[]) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const role = url.searchParams.get("role")
    const body =
      url.pathname === "/v1/models"
        ? CATALOG
        : url.pathname.endsWith("/prompts")
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

/**
 * The screen, with the two things around it a URL-driven screen needs to be
 * tested against: something that navigates the way the command palette does
 * (and back, the way the window's own Back button does), and the search string
 * the screen has left behind.
 */
function renderScreen(entry: string = paths.profiles()) {
  function Harness() {
    const navigate = useNavigate()
    const location = useLocation()
    return (
      <>
        <button type="button" onClick={() => void navigate(paths.profile(ENGINEER.id))}>
          pick Builder
        </button>
        <button type="button" onClick={() => void navigate(-1)}>
          go back
        </button>
        <output data-testid="search">{location.search}</output>
      </>
    )
  }

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  })
  return render(
    <MemoryRouter initialEntries={[entry]}>
      <QueryClientProvider client={queryClient}>
        <Harness />
        <ProfilesPage />
      </QueryClientProvider>
    </MemoryRouter>,
  )
}

/** What the screen has put in the URL, as the harness above reports it. */
function currentSearch(): string {
  return screen.getByTestId("search").textContent ?? ""
}

beforeEach(() => {
  // jsdom lays nothing out, so it does not implement this.
  Element.prototype.scrollIntoView = vi.fn()
  daemonFetch.mockReset()
  stubDaemon([ENGINEER, REVIEWER, PINNED])
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

  it("takes the param off the URL when the row is closed, so it stays closed", async () => {
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
    expect(currentSearch()).toBe("")
  })

  it("walks expansions with Back, since each one is a history step", async () => {
    const user = userEvent.setup()
    renderScreen()
    await screen.findByRole("button", { name: "Builder" })

    await user.click(screen.getByRole("button", { name: "pick Builder" }))
    await screen.findByRole("button", { name: "Builder", expanded: true })

    // Expanding another row from there is a step of its own, so Back is the
    // way from that one to the profile the link opened.
    await user.click(screen.getByRole("button", { name: "Critic" }))
    await screen.findByRole("button", { name: "Critic", expanded: true })

    await user.click(screen.getByRole("button", { name: "go back" }))
    expect(await screen.findByRole("button", { name: "Builder", expanded: true })).toBeDefined()
  })
})

describe("ProfilesPage, on a reload", () => {
  it("comes back up on the role tab and the row the URL names", async () => {
    renderScreen(`/profiles?role=reviewer&${PROFILE_EXPAND_PARAM}=${REVIEWER.id}`)

    expect(await screen.findByRole("button", { name: "Critic", expanded: true })).toBeDefined()
    expect(screen.getByRole("tab", { name: "Reviewer", selected: true })).toBeDefined()
    // The tab is a real filter, not just a selected trigger: it is what the
    // list was asked for.
    expect(screen.queryByRole("button", { name: "Builder" })).toBeNull()
  })

  it("keeps the picked tab in the URL, and the expansion with it", async () => {
    const user = userEvent.setup()
    renderScreen()
    await screen.findByRole("button", { name: "Critic" })

    await user.click(screen.getByRole("button", { name: "Critic" }))
    await screen.findByRole("button", { name: "Critic", expanded: true })
    await user.click(screen.getByRole("tab", { name: "Reviewer" }))

    await waitFor(() => {
      expect(new URLSearchParams(currentSearch()).get("role")).toBe("reviewer")
    })
    expect(new URLSearchParams(currentSearch()).get(PROFILE_EXPAND_PARAM)).toBe(REVIEWER.id)
    expect(screen.getByRole("button", { name: "Critic", expanded: true })).toBeDefined()
  })
})

describe("ProfilesPage, expanded details", () => {
  it("captions a catalog model with its capability blurb, and an unknown one with nothing", async () => {
    const user = userEvent.setup()
    renderScreen()

    // A stored model the catalog lists gets its description under it — a line
    // of its own, so the blurb is not cut short by the model's own truncation.
    await user.click(await screen.findByRole("button", { name: "Pinned" }))
    const blurb = await screen.findByText("Opus tier: deep analysis")
    expect(blurb.closest("dd")?.textContent).toBe("Opus tier: deep analysis")

    // One the catalog does not — here, no model at all — shows only itself.
    await user.click(screen.getByRole("button", { name: "Pinned" }))
    await user.click(screen.getByRole("button", { name: "Builder" }))
    await screen.findByRole("button", { name: "Builder", expanded: true })
    expect(screen.queryByText("Opus tier: deep analysis")).toBeNull()
  })

  it("shows no raw profile id: the name is what a profile is named by", async () => {
    const user = userEvent.setup()
    renderScreen()

    await user.click(await screen.findByRole("button", { name: "Pinned" }))
    await screen.findByRole("button", { name: "Pinned", expanded: true })

    expect(screen.queryByText(PINNED.id)).toBeNull()
    expect(screen.queryByRole("button", { name: "Copy profile id" })).toBeNull()
  })
})
