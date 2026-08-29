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

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useLocation, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, it } from "vitest"

import type { ModelDto, ProfileDto } from "@/api"
import { PROFILE_EXPAND_PARAM, paths } from "@/routes/paths"
import { aProfile } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { ProfilesPage } from "./profiles-page"

const ENGINEER: ProfileDto = aProfile({
  id: "01JPROF000000000000000ENG",
  name: "Builder",
})

const REVIEWER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000REV",
  name: "Critic",
  role: "reviewer",
}

/** A profile pinned to a model the catalog below knows about, at an effort. */
const PINNED: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000PIN",
  name: "Pinned",
  model: "claude_code:claude-opus-5",
  effort: "xhigh",
}

/** The same model, run at whatever the agent CLI runs it at. */
const AT_AUTO: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF00000000000000AUT",
  name: "Unhurried",
  model: "claude_code:claude-opus-5",
}

/** The model catalog an expanded row asks for, to caption a known model with. */
const CATALOG: ModelDto[] = [
  {
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
    efforts: ["low", "medium", "high", "xhigh", "max"],
    default_effort: "high",
  },
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
    return Promise.resolve(jsonResponse(body))
  })
}

/**
 * The screen, with the two things around it a URL-driven screen needs to be
 * tested against: something that navigates the way the command palette does
 * (and back, the way the window's own Back button does), and the search string
 * the screen has left behind.
 */
function renderPage(entry: string = paths.profiles()) {
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

  return renderScreen(
    <>
      <Harness />
      <ProfilesPage />
    </>,
    { route: entry },
  )
}

/** The panel a row expands into, which the row names through `aria-controls`. */
function expandedDetails(row: HTMLElement): HTMLElement {
  const id = row.getAttribute("aria-controls")
  const details = id ? document.getElementById(id) : null
  if (!details) throw new Error("the row is not expanded")
  return details
}

/** What the screen has put in the URL, as the harness above reports it. */
function currentSearch(): string {
  return screen.getByTestId("search").textContent ?? ""
}

beforeEach(() => {
  stubDaemon([ENGINEER, REVIEWER, PINNED, AT_AUTO])
})

// Testing Library only unmounts by itself under `globals: true`, which this
// project does not use — without this every screen stays in the document.

describe("ProfilesPage, on ?expand=", () => {
  it("expands the profile that was asked for and scrolls to it", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("button", { name: "Builder" })

    await user.click(screen.getByRole("button", { name: "pick Builder" }))

    const row = await screen.findByRole("button", { name: "Builder", expanded: true })
    expect(row).toBeDefined()
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled()
  })

  it("widens the role tab, so a pick is not filtered out of the list it lands in", async () => {
    const user = userEvent.setup()
    renderPage()
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
    renderPage()
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
    renderPage()
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
    renderPage(`/profiles?role=reviewer&${PROFILE_EXPAND_PARAM}=${REVIEWER.id}`)

    expect(await screen.findByRole("button", { name: "Critic", expanded: true })).toBeDefined()
    expect(screen.getByRole("tab", { name: "Reviewer", selected: true })).toBeDefined()
    // The tab is a real filter, not just a selected trigger: it is what the
    // list was asked for.
    expect(screen.queryByRole("button", { name: "Builder" })).toBeNull()
  })

  it("keeps the picked tab in the URL, and the expansion with it", async () => {
    const user = userEvent.setup()
    renderPage()
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

describe("ProfilesPage, the model column", () => {
  it("shows the qualified id, which is where the agent CLI is now named", async () => {
    renderPage()

    const row = (await screen.findByRole("button", { name: "Pinned" })).closest("tr")
    expect(row?.textContent).toContain("claude_code:claude-opus-5")
    // The CLI is the first half of that id, so it is no column of its own.
    expect(screen.queryByRole("columnheader", { name: "Agent" })).toBeNull()
    expect(screen.getByRole("columnheader", { name: "Model" })).toBeDefined()
  })

  it("says `auto` where a profile pins nothing, rather than leaving a blank", async () => {
    renderPage()

    const row = (await screen.findByRole("button", { name: "Builder" })).closest("tr")
    expect(row?.textContent).toContain("auto")
  })

  it("puts the effort in the same cell, after an `@`", async () => {
    renderPage()

    const row = (await screen.findByRole("button", { name: "Pinned" })).closest("tr")
    expect(row?.textContent).toContain("claude_code:claude-opus-5 @ xhigh")
  })

  it("adds nothing where no effort is pinned: that is the agent CLI's own", async () => {
    renderPage()

    const row = (await screen.findByRole("button", { name: "Unhurried" })).closest("tr")
    expect(row?.textContent).toContain("claude_code:claude-opus-5")
    expect(row?.textContent).not.toContain("@")
  })
})

describe("ProfilesPage, expanded details", () => {
  it("spells the pin out whole: the model, and the effort it is run at", async () => {
    const user = userEvent.setup()
    renderPage()

    const row = await screen.findByRole("button", { name: "Pinned" })
    await user.click(row)

    expect(expandedDetails(row).textContent).toContain("claude_code:claude-opus-5 @ xhigh")
  })

  /**
   * The details row is the one place an unpinned effort is a word rather than
   * nothing: the row exists to say what this profile runs as, and `auto` — with
   * what the CLI does instead beside it — is that answer, exactly as the model
   * above it reads.
   */
  it("says `auto` for an unpinned effort, named with what the CLI runs it at", async () => {
    const user = userEvent.setup()
    renderPage()

    const row = await screen.findByRole("button", { name: "Unhurried" })
    await user.click(row)

    expect(expandedDetails(row).textContent).toContain("claude_code:claude-opus-5 @ auto (high)")
  })

  it("captions a catalog model with its capability blurb, and an unknown one with nothing", async () => {
    const user = userEvent.setup()
    renderPage()

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

  /**
   * `truncate` is `overflow: hidden` plus `text-overflow: ellipsis`, and an
   * inline box applies neither: the word and the sentence after it are one
   * line, and on an inline span that line paints straight out of the fact's
   * column instead of ending in an ellipsis. What is cut off is in the hint
   * this fact already carries, so the cut is the point.
   */
  it("cuts the unpinned model at its column rather than painting past it", async () => {
    const user = userEvent.setup()
    renderPage()

    await user.click(await screen.findByRole("button", { name: "Builder" }))
    await screen.findByRole("button", { name: "Builder", expanded: true })

    const said = await screen.findByText(/first installed CLI/)
    const line = said.parentElement
    expect(line?.textContent).toContain("auto")
    expect(line?.className).toContain("truncate")
    // The half that makes the ellipsis possible at all.
    expect(line?.className).toContain("block")
  })

  it("shows no raw profile id: the name is what a profile is named by", async () => {
    const user = userEvent.setup()
    renderPage()

    await user.click(await screen.findByRole("button", { name: "Pinned" }))
    await screen.findByRole("button", { name: "Pinned", expanded: true })

    expect(screen.queryByText(PINNED.id)).toBeNull()
    expect(screen.queryByRole("button", { name: "Copy profile id" })).toBeNull()
  })
})
