// @vitest-environment jsdom

/**
 * The profiles screen as a list beside an editor, against a stubbed daemon.
 *
 * What is checked is the screen's own state rather than the daemon's: the
 * list is grouped by role, the selection lives in the URL as `?profile=` —
 * the link the command palette follows when a profile is picked — and a
 * selection is a history step, so Back returns to the one before it. Under a
 * data router, because the editor guards unsaved edits with the router's own
 * blocker, and switching profiles is the one guard this file owns: the editor
 * itself is `profile-editor.test.tsx`'s.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider, useLocation, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, it } from "vitest"

import type { ModelDto, ProfileDto } from "@/api"
import { PROFILE_PARAM, paths } from "@/routes/paths"
import { aModel, anEffort, aProfile } from "@/test/fixtures"
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

const PLANNER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000PLN",
  name: "Mapper",
  role: "planner",
}

/** A profile pinned to a model the catalog below knows about, at an effort. */
const PINNED: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000PIN",
  name: "Pinned",
  model: "claude_code:claude-opus-5",
  effort: "xhigh",
}

const CATALOG: ModelDto[] = [
  aModel({
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
    tier: "strong",
    efforts: [
      anEffort({ id: "low" }),
      anEffort({ id: "medium" }),
      anEffort({ id: "high", default: true }),
      anEffort({ id: "xhigh" }),
      anEffort({ id: "max" }),
    ],
  }),
]

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []

/**
 * The daemon: the list, the catalog, each profile's briefings, and the writes
 * the screen can make — a create that then shows up in the list, a delete
 * that takes its row away, and the prompt write the dirty guard needs.
 */
function stubDaemon(initial: ProfileDto[]) {
  const profiles = [...initial]
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (pathname === "/v1/models") return jsonResponse(CATALOG)
    if (pathname === "/v1/profiles" && request.method === "GET") return jsonResponse(profiles)
    if (pathname === "/v1/profiles" && request.method === "POST") {
      const created: ProfileDto = { ...ENGINEER, ...body, id: "01JPROF00000000000000NEW" }
      profiles.push(created)
      return jsonResponse(created, 201)
    }
    const one = pathname.match(/^\/v1\/profiles\/([^/]+)$/)
    if (one && request.method === "DELETE") {
      const index = profiles.findIndex((profile) => profile.id === one[1])
      if (index >= 0) profiles.splice(index, 1)
      return new Response(null, { status: 204 })
    }
    if (one && request.method === "PUT") {
      const index = profiles.findIndex((profile) => profile.id === one[1])
      const updated = { ...profiles[index], ...body } as ProfileDto
      profiles[index] = updated
      return jsonResponse(updated)
    }
    return new Response("not stubbed", { status: 404 })
  })
}

/**
 * The screen under a data router — the app's kind, and the kind the editor's
 * leave guard needs — with something beside it that navigates the way the
 * command palette does (and back, the way the window's Back button does), and
 * the search string the screen has left behind.
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
  const router = createMemoryRouter(
    [
      {
        path: paths.profiles(),
        element: (
          <>
            <Harness />
            <ProfilesPage />
          </>
        ),
      },
    ],
    { initialEntries: [entry] },
  )
  return renderScreen(<RouterProvider router={router} />, { route: null })
}

/** What the screen has put in the URL, as the harness above reports it. */
function selectedInUrl(): string | null {
  return new URLSearchParams(screen.getByTestId("search").textContent ?? "").get(PROFILE_PARAM)
}

/** The list item of one profile, by the name it leads with. */
function item(name: string): HTMLElement {
  return screen.getByRole("link", { name: new RegExp(`^${name}`) })
}

/** The editor's heading, once the selected profile is up. */
async function editorFor(name: string): Promise<HTMLElement> {
  return await screen.findByRole("heading", { level: 2, name })
}

beforeEach(() => {
  requests = []
  stubDaemon([ENGINEER, REVIEWER, PLANNER, PINNED])
})

describe("the list", () => {
  it("groups every profile under its role, in the order the orchestration runs them", async () => {
    renderPage()
    await screen.findByRole("link", { name: /^Builder/ })

    const headings = screen
      .getAllByRole("heading", { level: 3 })
      .map((heading) => heading.textContent)
    expect(headings).toEqual(["Planner", "Engineer", "Reviewer"])

    const engineers = screen.getByRole("region", { name: "Engineer" })
    expect(within(engineers).getByRole("link", { name: /^Builder/ })).toBeDefined()
    expect(within(engineers).getByRole("link", { name: /^Pinned/ })).toBeDefined()
    expect(within(engineers).queryByRole("link", { name: /^Critic/ })).toBeNull()
  })

  it("says what each profile runs on, after its name", async () => {
    renderPage()
    await screen.findByRole("link", { name: /^Builder/ })

    expect(item("Pinned").textContent).toContain("claude_code:claude-opus-5 @ xhigh")
    // Nothing pinned is a fact about the profile, not a blank.
    expect(item("Builder").textContent).toContain("auto")
  })

  it("narrows by name from the filter box, and says so when nothing is left", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^Builder/ })

    await user.type(screen.getByRole("searchbox", { name: "Filter profiles" }), "crit")

    expect(screen.getByRole("link", { name: /^Critic/ })).toBeDefined()
    expect(screen.queryByRole("link", { name: /^Builder/ })).toBeNull()
    // A role with nothing left under it has no heading either.
    expect(screen.queryByRole("heading", { level: 3, name: "Engineer" })).toBeNull()

    await user.type(screen.getByRole("searchbox", { name: "Filter profiles" }), "ical")
    expect(screen.getByText("No profile is named that.")).toBeDefined()
  })

  it("offers to create the first profile when there are none", async () => {
    stubDaemon([])
    renderPage()

    expect(await screen.findByText("No profiles yet")).toBeDefined()
    expect(screen.getAllByRole("button", { name: "New profile" }).length).toBeGreaterThan(1)
  })
})

describe("the selection", () => {
  it("is nothing until a profile is picked, and says so", async () => {
    renderPage()
    await screen.findByRole("link", { name: /^Builder/ })

    expect(screen.getByText("Select a profile, or create one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("puts a picked profile in the URL and opens its editor", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^Builder/ })

    await user.click(item("Builder"))

    expect(await editorFor("Builder")).toBeDefined()
    expect(selectedInUrl()).toBe(ENGINEER.id)
    expect(item("Builder").getAttribute("aria-current")).toBe("page")
    expect(await screen.findByRole("textbox", { name: "System prompt" })).toBeDefined()
  })

  it("selects and scrolls to the profile a link asked for", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^Builder/ })

    await user.click(screen.getByRole("button", { name: "pick Builder" }))

    expect(await editorFor("Builder")).toBeDefined()
    expect(item("Builder").getAttribute("aria-current")).toBe("page")
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled()
  })

  it("comes back up on the profile the URL names, on a reload", async () => {
    renderPage(paths.profile(REVIEWER.id))

    expect(await editorFor("Critic")).toBeDefined()
    expect(await screen.findByRole("textbox", { name: "System prompt" })).toBeDefined()
  })

  it("walks selections with Back, since each one is a history step", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^Builder/ })

    await user.click(item("Builder"))
    await editorFor("Builder")
    await user.click(item("Critic"))
    await editorFor("Critic")

    await user.click(screen.getByRole("button", { name: "go back" }))
    expect(await editorFor("Builder")).toBeDefined()
    expect(selectedInUrl()).toBe(ENGINEER.id)

    // And from the first pick, Back is the list with nothing selected.
    await user.click(screen.getByRole("button", { name: "go back" }))
    expect(await screen.findByText("Select a profile, or create one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("clears from the editor's own way back, in place", async () => {
    const user = userEvent.setup()
    renderPage(paths.profile(ENGINEER.id))
    await editorFor("Builder")

    await user.click(screen.getByRole("button", { name: "Back to the list" }))

    expect(await screen.findByText("Select a profile, or create one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("says so when the URL names a profile the daemon does not have", async () => {
    renderPage(paths.profile("01JPROF000000000000000GON"))
    await screen.findByRole("link", { name: /^Builder/ })

    expect(screen.getByText("No profile by that id.")).toBeDefined()
    expect(screen.queryByRole("heading", { level: 2 })).toBeNull()
  })
})

describe("switching with unsaved edits", () => {
  it("asks first, and Cancel stays put with the edits intact", async () => {
    const user = userEvent.setup()
    renderPage(paths.profile(ENGINEER.id))
    await editorFor("Builder")

    const systemPrompt = await screen.findByRole("textbox", { name: "System prompt" })
    await user.type(systemPrompt, " More.")
    await user.click(item("Critic"))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Keep editing" }))

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Discard changes?" })).toBeNull()
    })
    expect(selectedInUrl()).toBe(ENGINEER.id)
    expect(screen.getByRole("heading", { level: 2, name: "Builder" })).toBeDefined()
    expect(
      (screen.getByRole("textbox", { name: "System prompt" }) as HTMLTextAreaElement).value,
    ).toContain(" More.")
    // Nothing was written by either answer.
    expect(requests.filter((one) => one.method !== "GET")).toEqual([])
  })

  it("switches on Discard, dropping the edits", async () => {
    const user = userEvent.setup()
    renderPage(paths.profile(ENGINEER.id))
    await editorFor("Builder")

    await user.type(await screen.findByRole("textbox", { name: "System prompt" }), " More.")
    await user.click(item("Critic"))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Discard" }))

    expect(await editorFor("Critic")).toBeDefined()
    expect(selectedInUrl()).toBe(REVIEWER.id)
    expect(requests.filter((one) => one.method !== "GET")).toEqual([])
  })

  it("lets a clean editor go without a word", async () => {
    const user = userEvent.setup()
    renderPage(paths.profile(ENGINEER.id))
    await editorFor("Builder")
    await screen.findByRole("textbox", { name: "System prompt" })

    await user.click(item("Critic"))

    expect(await editorFor("Critic")).toBeDefined()
    expect(screen.queryByRole("dialog", { name: "Discard changes?" })).toBeNull()
  })
})

describe("creating and deleting", () => {
  /**
   * The dialog's own fields are `create-profile-dialog.test.tsx`'s; what is
   * the screen's is where the new profile lands.
   */
  it("creates a profile from the small dialog and lands on it", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^Builder/ })

    await user.click(screen.getByRole("button", { name: "New profile" }))
    const dialog = await screen.findByRole("dialog", { name: "New profile" })
    await user.type(within(dialog).getByLabelText("Name"), "rust-reviewer")
    await user.click(within(dialog).getByLabelText("Role"))
    await user.click(await screen.findByRole("option", { name: "Reviewer" }))
    await user.click(within(dialog).getByRole("button", { name: "Create profile" }))

    await waitFor(() => {
      expect(requests.find((one) => one.method === "POST")?.body).toMatchObject({
        name: "rust-reviewer",
        role: "reviewer",
      })
    })
    // The editor opens on the new profile.
    expect(await editorFor("rust-reviewer")).toBeDefined()
    expect(selectedInUrl()).toBe("01JPROF00000000000000NEW")
    expect(await screen.findByRole("textbox", { name: "System prompt" })).toBeDefined()
    expect(
      within(screen.getByRole("region", { name: "Reviewer" })).getByRole("link", {
        name: /^rust-reviewer/,
      }),
    ).toBeDefined()
  })

  it("deletes the selected profile from the editor's header and clears the selection", async () => {
    const user = userEvent.setup()
    renderPage(paths.profile(REVIEWER.id))
    await editorFor("Critic")

    await user.click(screen.getByRole("button", { name: "Delete" }))
    const dialog = await screen.findByRole("dialog", { name: "Delete “Critic”?" })
    await user.click(within(dialog).getByRole("button", { name: "Delete profile" }))

    await waitFor(() => {
      expect(screen.queryByRole("link", { name: /^Critic/ })).toBeNull()
    })
    expect(requests.some((one) => one.method === "DELETE")).toBe(true)
    expect(selectedInUrl()).toBeNull()
    expect(screen.getByText("Select a profile, or create one.")).toBeDefined()
  })
})
