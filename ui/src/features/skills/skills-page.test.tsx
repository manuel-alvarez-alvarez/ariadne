// @vitest-environment jsdom

/**
 * The skills screen as a list beside an editor, against a stubbed daemon.
 *
 * What is checked is the screen's own state rather than the daemon's: the list
 * is grouped by where the document came from — which is what says whether the
 * skill is reset or deleted — and the selection lives in the URL as `?skill=`,
 * the link every mention of a skill points at. A selection is a history step,
 * so Back returns to the one before it.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider, useLocation, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, it } from "vitest"

import type { SkillDto } from "@/api"
import { paths, SKILL_PARAM } from "@/routes/paths"
import { aSkill } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SkillsPage } from "./skills-page"

const CODING: SkillDto = aSkill({ name: "coding" })

const REVIEW: SkillDto = aSkill({
  name: "code-review",
  summary: "Read a change for the defects it carries.",
  document: "---\nname: code-review\n---\nRead it.",
})

/** A shipped skill somebody has written over: the one row that gets a badge. */
const EDITED: SkillDto = aSkill({
  name: "release",
  summary: "Cut a release.",
  document_is_default: false,
})

/** A skill of the user's own: deleted rather than reset. */
const MINE: SkillDto = aSkill({
  name: "api-design",
  summary: "Design an HTTP interface.",
  document: "---\nname: api-design\n---\nDesign it.",
  builtin: false,
  document_is_default: false,
})

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []

/**
 * The daemon: the list, and the writes the screen can make — a create that
 * then shows up in the list, and a delete that takes its row away.
 */
function stubDaemon(initial: SkillDto[]) {
  const skills = [...initial]
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (pathname === "/v1/skills" && request.method === "GET") return jsonResponse(skills)
    if (pathname === "/v1/skills" && request.method === "POST") {
      const created: SkillDto = {
        ...MINE,
        ...body,
        summary: "A skill of my own.",
        builtin: false,
        document_is_default: false,
      }
      skills.push(created)
      return jsonResponse(created, 201)
    }
    const one = pathname.match(/^\/v1\/skills\/([^/]+)$/)
    if (one && request.method === "DELETE") {
      const index = skills.findIndex((skill) => skill.name === one[1])
      if (index >= 0) skills.splice(index, 1)
      return new Response(null, { status: 204 })
    }
    return new Response("not stubbed", { status: 404 })
  })
}

/**
 * The screen under a data router, with something beside it that navigates the
 * way a link to a skill does (and back, the way the window's Back button
 * does), and the search string the screen has left behind.
 */
function renderPage(entry: string = paths.skills()) {
  function Harness() {
    const navigate = useNavigate()
    const location = useLocation()
    return (
      <>
        <button type="button" onClick={() => void navigate(paths.skill(REVIEW.name))}>
          pick code-review
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
        path: paths.skills(),
        element: (
          <>
            <Harness />
            <SkillsPage />
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
  return new URLSearchParams(screen.getByTestId("search").textContent ?? "").get(SKILL_PARAM)
}

/** The list item of one skill, by the name it leads with. */
function item(name: string): HTMLElement {
  return screen.getByRole("link", { name: new RegExp(`^${name}`) })
}

/** The editor's heading, once the selected skill is up. */
async function editorFor(name: string): Promise<HTMLElement> {
  return await screen.findByRole("heading", { level: 2, name })
}

beforeEach(() => {
  requests = []
  stubDaemon([CODING, REVIEW, EDITED, MINE])
})

describe("the list", () => {
  it("groups the shipped skills apart from the ones you wrote", async () => {
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    const headings = screen
      .getAllByRole("heading", { level: 3 })
      .map((heading) => heading.textContent)
    expect(headings).toEqual(["Shipped with Ariadne", "Yours"])

    const shipped = screen.getByRole("region", { name: "Shipped with Ariadne" })
    expect(within(shipped).getByRole("link", { name: /^coding/ })).toBeDefined()
    expect(within(shipped).queryByRole("link", { name: /^api-design/ })).toBeNull()
    expect(
      within(screen.getByRole("region", { name: "Yours" })).getByRole("link", {
        name: /^api-design/,
      }),
    ).toBeDefined()
  })

  it("says what each skill is for, under its name", async () => {
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    expect(item("coding").textContent).toContain(CODING.summary)
  })

  it("badges only a shipped skill somebody has rewritten", async () => {
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    expect(item("release").textContent).toContain("edited")
    // The two that behave as expected say nothing: a shipped skill on its own
    // text, and a skill of yours, which has no shipped text to differ from.
    expect(item("coding").textContent).not.toContain("edited")
    expect(item("api-design").textContent).not.toContain("edited")
  })

  it("narrows by name or by what the skill is for, and says so when nothing is left", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    const filter = screen.getByRole("textbox", { name: "Filter skills" })
    await user.type(filter, "defects")

    // Matched on its summary, not its name.
    expect(screen.getByRole("link", { name: /^code-review/ })).toBeDefined()
    expect(screen.queryByRole("link", { name: /^coding/ })).toBeNull()
    // A group with nothing left under it has no heading either.
    expect(screen.queryByRole("heading", { level: 3, name: "Yours" })).toBeNull()

    await user.clear(filter)
    await user.type(filter, "nothing")
    expect(screen.getByText("No skill matches “nothing”.")).toBeDefined()
  })
})

describe("the selection", () => {
  it("is nothing until a skill is picked, and says so", async () => {
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    expect(screen.getByText("Select a skill, or write one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("puts a picked skill in the URL and opens its document", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    await user.click(item("coding"))

    expect(await editorFor("coding")).toBeDefined()
    expect(selectedInUrl()).toBe("coding")
    expect(item("coding").getAttribute("aria-current")).toBe("true")
    expect((screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement).value).toBe(
      CODING.document,
    )
  })

  it("opens on the skill a link named, so a mention of one leads here", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    await user.click(screen.getByRole("button", { name: "pick code-review" }))

    expect(await editorFor("code-review")).toBeDefined()
    expect(item("code-review").getAttribute("aria-current")).toBe("true")
  })

  it("comes back up on the skill the URL names, on a reload", async () => {
    renderPage(paths.skill(REVIEW.name))

    expect(await editorFor("code-review")).toBeDefined()
    expect((screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement).value).toBe(
      REVIEW.document,
    )
  })

  it("walks selections with Back, since each one is a history step", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    await user.click(item("coding"))
    await editorFor("coding")
    await user.click(item("code-review"))
    await editorFor("code-review")

    await user.click(screen.getByRole("button", { name: "go back" }))
    expect(await editorFor("coding")).toBeDefined()
    expect(selectedInUrl()).toBe("coding")

    // And from the first pick, Back is the list with nothing selected.
    await user.click(screen.getByRole("button", { name: "go back" }))
    expect(await screen.findByText("Select a skill, or write one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("says so when the URL names a skill the daemon does not have", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill("gone-since"))
    await screen.findByRole("link", { name: /^coding/ })

    expect(screen.getByText("No skill by that name.")).toBeDefined()
    expect(screen.queryByRole("heading", { level: 2 })).toBeNull()

    // And the way back clears it in place, rather than as another step.
    await user.click(screen.getByRole("button", { name: "Back to the list" }))
    expect(await screen.findByText("Select a skill, or write one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("shows one skill's document under one skill's name, never the last one's", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(CODING.name))
    await editorFor("coding")

    await user.type(screen.getByRole("textbox", { name: "Document" }), "typed over it")
    await user.click(item("code-review"))

    await editorFor("code-review")
    expect((screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement).value).toBe(
      REVIEW.document,
    )
  })
})

describe("writing and deleting", () => {
  /**
   * The dialog's own fields are `create-skill-dialog.test.tsx`'s; what is the
   * screen's is where the new skill lands.
   */
  it("writes a skill from the dialog and opens it", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("link", { name: /^coding/ })

    await user.click(screen.getByRole("button", { name: "New skill" }))
    const dialog = await screen.findByRole("dialog", { name: "New skill" })
    await user.type(within(dialog).getByLabelText("Name"), "api-review")
    await user.click(within(dialog).getByRole("button", { name: "Create skill" }))

    await waitFor(() => {
      expect(requests.find((one) => one.method === "POST")?.body).toMatchObject({
        name: "api-review",
      })
    })
    expect(await editorFor("api-review")).toBeDefined()
    expect(selectedInUrl()).toBe("api-review")
    expect(
      within(screen.getByRole("region", { name: "Yours" })).getByRole("link", {
        name: /^api-review/,
      }),
    ).toBeDefined()
  })

  it("deletes the selected skill and clears the selection", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(MINE.name))
    await editorFor("api-design")

    await user.click(screen.getByRole("button", { name: "Delete" }))
    const dialog = await screen.findByRole("dialog", { name: "Delete api-design?" })
    await user.click(within(dialog).getByRole("button", { name: "Delete" }))

    await waitFor(() => {
      expect(screen.queryByRole("link", { name: /^api-design/ })).toBeNull()
    })
    expect(requests.some((one) => one.method === "DELETE")).toBe(true)
    expect(selectedInUrl()).toBeNull()
    expect(screen.getByText("Select a skill, or write one.")).toBeDefined()
  })
})
