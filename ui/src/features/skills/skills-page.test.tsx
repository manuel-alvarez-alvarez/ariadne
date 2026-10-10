// @vitest-environment jsdom

/**
 * The skills screen as a combobox beside an editor, against a stubbed
 * daemon. The catalog sits behind the trigger rather than down the side, so
 * what is checked is the picker's own behaviour: it groups by where a skill
 * came from — which is what says whether it is reset or deleted — it is
 * filtered by name or by what the skill says it is for, and a pick lands in
 * the URL as `?skill=`, the link every mention of a skill points at.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createBrowserRouter, RouterProvider, useLocation, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, it } from "vitest"

import type { SkillDto } from "@/api"
import { paths, SKILL_PARAM } from "@/routes/paths"
import { aSkill } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen, startAt } from "@/test/harness"
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
 * The daemon: the catalog, and the writes the screen can make — a create that
 * then shows up in the picker, and a delete that takes its row away.
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
 * way a mention of a skill does (and back, the way the window's Back button
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
        <button type="button" onClick={() => void navigate(paths.agents())}>
          go to agents
        </button>
        <output data-testid="search">{location.search}</output>
      </>
    )
  }
  startAt(entry)
  const router = createBrowserRouter([
    {
      path: paths.skills(),
      element: (
        <>
          <Harness />
          <SkillsPage />
        </>
      ),
    },
    // Another screen entirely, the way a sidebar link would open one: not a
    // skill selection, so no `SKILL_PARAM` is involved in leaving to it.
    { path: paths.agents(), element: <p>Agents screen</p> },
  ])
  return renderScreen(<RouterProvider router={router} />, { route: null })
}

/** What the screen has put in the URL, as the harness above reports it. */
function selectedInUrl(): string | null {
  return new URLSearchParams(screen.getByTestId("search").textContent ?? "").get(SKILL_PARAM)
}

/** The combobox trigger, which also reads back as the whole choice. */
function trigger(): HTMLElement {
  return screen.getByRole("button", { name: "Skill" })
}

async function openCombobox(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Skill" }))
  await screen.findByRole("listbox", { name: "Skill" })
}

/** The group of one title, found by its own accessible name. */
function group(title: string): HTMLElement {
  return screen.getByRole("group", { name: title })
}

/** A row in the open popup, by the name it leads with. */
function row(name: string): HTMLElement {
  return screen.getByRole("option", { name: new RegExp(`^${name}`) })
}

/** The editor's heading, once the selected skill is up. */
async function editorFor(name: string): Promise<HTMLElement> {
  return await screen.findByRole("heading", { level: 2, name })
}

beforeEach(() => {
  requests = []
  stubDaemon([CODING, REVIEW, EDITED, MINE])
})

describe("the combobox", () => {
  it("shows no left list panel", async () => {
    renderPage()
    await screen.findByRole("button", { name: "Skill" })

    expect(screen.queryByRole("navigation")).toBeNull()
  })

  it("groups the shipped skills apart from the ones you wrote", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    expect(
      within(group("Shipped with Ariadne")).getByRole("option", { name: /^coding/ }),
    ).toBeDefined()
    expect(
      within(group("Shipped with Ariadne")).queryByRole("option", { name: /^api-design/ }),
    ).toBeNull()
    expect(within(group("Yours")).getByRole("option", { name: /^api-design/ })).toBeDefined()
  })

  it("says what each skill is for, under its name", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    expect(row("coding").textContent).toContain(CODING.summary)
  })

  it("badges only a shipped skill somebody has rewritten", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    expect(row("release").textContent).toContain("edited")
    // The two that behave as expected say nothing: a shipped skill on its own
    // text, and a skill of yours, which has no shipped text to differ from.
    expect(row("coding").textContent).not.toContain("edited")
    expect(row("api-design").textContent).not.toContain("edited")
  })

  it("narrows by name or by what the skill is for, and says so when nothing is left", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    const filter = screen.getByRole("combobox", { name: "Skill" })
    await user.type(filter, "defects")

    // Matched on its summary, not its name.
    expect(screen.getByRole("option", { name: /^code-review/ })).toBeDefined()
    expect(screen.queryByRole("option", { name: /^coding/ })).toBeNull()
    // A group with nothing left under it has no heading either.
    expect(screen.queryByRole("group", { name: "Yours" })).toBeNull()

    await user.clear(filter)
    await user.type(filter, "nothing")
    expect(screen.getByText("No skill matches “nothing”.")).toBeDefined()
  })
})

describe("the selection", () => {
  it("is nothing until a skill is picked, and says so", async () => {
    renderPage()
    await screen.findByRole("button", { name: "Skill" })

    expect(screen.getByText("Select a skill, or write one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("puts a picked skill in the URL and opens its document", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    await user.click(row("coding"))

    expect(await editorFor("coding")).toBeDefined()
    expect(selectedInUrl()).toBe("coding")
    expect(trigger().textContent).toContain("coding")
    expect((screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement).value).toBe(
      CODING.document,
    )
  })

  it("opens on the skill a link named, so a mention of one leads here", async () => {
    const user = userEvent.setup()
    renderPage()
    await screen.findByRole("button", { name: "Skill" })

    await user.click(screen.getByRole("button", { name: "pick code-review" }))

    expect(await editorFor("code-review")).toBeDefined()
    expect(trigger().textContent).toContain("code-review")
  })

  it("picks with the keyboard: open, filter, enter", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    await user.keyboard("api-design")
    await user.keyboard("{Enter}")

    expect(await editorFor("api-design")).toBeDefined()
    expect(selectedInUrl()).toBe("api-design")
  })

  it("closes on Escape, leaving the selection as it was", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)

    await user.keyboard("{Escape}")

    await waitFor(() => {
      expect(screen.queryByRole("listbox", { name: "Skill" })).toBeNull()
    })
    expect(selectedInUrl()).toBeNull()
  })

  it("comes back up on the skill the URL names, on a reload", async () => {
    renderPage(paths.skill(REVIEW.name))

    expect(await editorFor("code-review")).toBeDefined()
    expect((screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement).value).toBe(
      REVIEW.document,
    )
  })

  it("says so when the URL names a skill the daemon does not have", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill("gone-since"))
    await screen.findByRole("button", { name: "Skill" })

    expect(screen.getByText("No skill by that name.")).toBeDefined()
    expect(screen.queryByRole("heading", { level: 2 })).toBeNull()

    // And the way back clears it in place, rather than as another step.
    await user.click(screen.getByRole("button", { name: "Clear selection" }))
    expect(await screen.findByText("Select a skill, or write one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("shows one skill's document under one skill's name, never the last one's", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(CODING.name))
    await editorFor("coding")

    await openCombobox(user)
    await user.click(row("code-review"))

    await editorFor("code-review")
    expect((screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement).value).toBe(
      REVIEW.document,
    )
  })
})

describe("leaving a dirty skill", () => {
  it("asks before switching to a different skill from the combobox", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(CODING.name))
    await editorFor("coding")

    const textarea = screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement
    await user.type(textarea, "typed over it")
    await openCombobox(user)
    await user.click(row("code-review"))

    expect(await screen.findByRole("dialog", { name: "Discard changes?" })).toBeDefined()
    // Blocked: the selection has not moved, and the box still has what was
    // typed, once the dialog in front of it is answered.
    expect(selectedInUrl()).toBe("coding")
    expect(textarea.value).toBe(`${CODING.document}typed over it`)
  })

  it("keeps the draft and stays, on Keep editing", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(CODING.name))
    await editorFor("coding")

    await user.type(screen.getByRole("textbox", { name: "Document" }), "typed over it")
    await openCombobox(user)
    await user.click(row("code-review"))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Keep editing" }))

    expect(screen.queryByRole("dialog", { name: "Discard changes?" })).toBeNull()
    expect(await editorFor("coding")).toBeDefined()
    expect(selectedInUrl()).toBe("coding")
    expect((screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement).value).toBe(
      `${CODING.document}typed over it`,
    )
  })

  it("discards the draft and switches, on Discard", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(CODING.name))
    await editorFor("coding")

    await user.type(screen.getByRole("textbox", { name: "Document" }), "typed over it")
    await openCombobox(user)
    await user.click(row("code-review"))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Discard" }))

    expect(await editorFor("code-review")).toBeDefined()
    expect(selectedInUrl()).toBe("code-review")
    expect((screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement).value).toBe(
      REVIEW.document,
    )
  })

  it("asks before Back leaves a dirty skill, too", async () => {
    const user = userEvent.setup()
    renderPage()
    await openCombobox(user)
    await user.click(row("coding"))
    await editorFor("coding")

    await user.type(screen.getByRole("textbox", { name: "Document" }), "typed over it")
    await user.click(screen.getByRole("button", { name: "go back" }))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Discard" }))

    expect(await screen.findByText("Select a skill, or write one.")).toBeDefined()
    expect(selectedInUrl()).toBeNull()
  })

  it("asks before a route to another screen leaves a dirty skill, too", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(CODING.name))
    await editorFor("coding")

    await user.type(screen.getByRole("textbox", { name: "Document" }), "typed over it")
    await user.click(screen.getByRole("button", { name: "go to agents" }))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Discard" }))

    expect(await screen.findByText("Agents screen")).toBeDefined()
  })

  it("leaves a clean skill with no prompt", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(CODING.name))
    await editorFor("coding")

    await openCombobox(user)
    await user.click(row("code-review"))

    expect(await editorFor("code-review")).toBeDefined()
    expect(screen.queryByRole("dialog", { name: "Discard changes?" })).toBeNull()
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
    await screen.findByRole("button", { name: "Skill" })

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
    expect(trigger().textContent).toContain("api-review")
    await openCombobox(user)
    expect(within(group("Yours")).getByRole("option", { name: /^api-review/ })).toBeDefined()
  })

  it("deletes the selected skill and clears the selection", async () => {
    const user = userEvent.setup()
    renderPage(paths.skill(MINE.name))
    await editorFor("api-design")

    await user.click(screen.getByRole("button", { name: "Delete" }))
    const dialog = await screen.findByRole("dialog", { name: "Delete api-design?" })
    await user.click(within(dialog).getByRole("button", { name: "Delete" }))

    // The catalog drops the row and the editor clears the selection in two
    // steps, so a slow run can see the first without the second yet.
    await waitFor(() => {
      expect(selectedInUrl()).toBeNull()
    })
    expect(requests.some((one) => one.method === "DELETE")).toBe(true)
    expect(screen.getByText("Select a skill, or write one.")).toBeDefined()
    await openCombobox(user)
    expect(screen.queryByRole("option", { name: /^api-design/ })).toBeNull()
  })
})
