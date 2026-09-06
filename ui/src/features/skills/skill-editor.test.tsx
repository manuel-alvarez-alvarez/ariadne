// @vitest-environment jsdom

/**
 * One skill's document, and the one thing that can be done to it besides
 * saving.
 *
 * The rule this file holds is the asymmetry: a skill Ariadne ships is *reset*
 * and cannot be deleted, and a skill of the user's own is *deleted* and cannot
 * be reset, because nothing ships under its name to go back to. The daemon
 * refuses the wrong one either way; what is checked here is that the screen
 * never offers it.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { SkillDto } from "@/api"
import { aSkill } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { SkillEditor } from "./skill-editor"

const SHIPPED: SkillDto = aSkill({ name: "coding" })

/** A shipped skill somebody has written over: the one that can be reset. */
const REWRITTEN: SkillDto = aSkill({
  name: "coding",
  document: "---\nname: coding\n---\nMine now.",
  document_is_default: false,
})

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

function stubDaemon(answer: (path: string, method: string) => Response) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    requests.push({
      method: request.method,
      path: pathname,
      body: raw.length > 0 ? JSON.parse(raw) : null,
    })
    return answer(pathname, request.method)
  })
}

function renderEditor(skill: SkillDto, onDeleted = vi.fn()) {
  renderScreen(<SkillEditor skill={skill} onDeleted={onDeleted} />)
  return { onDeleted }
}

function box(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement
}

beforeEach(() => {
  requests = []
  stubDaemon(() => jsonResponse(SHIPPED))
})

describe("the document", () => {
  it("opens on the skill's own text, and says where that text came from", () => {
    renderEditor(SHIPPED)

    expect(box().value).toBe(SHIPPED.document)
    expect(screen.getByText("shipped")).toBeDefined()
    expect(screen.getByText(SHIPPED.summary)).toBeDefined()
  })

  it("says a shipped skill has been written over, rather than calling it shipped", () => {
    renderEditor(REWRITTEN)
    expect(screen.getByText("shipped · edited")).toBeDefined()
  })

  it("saves nothing until the text differs, and sends the whole document", async () => {
    const user = userEvent.setup()
    renderEditor(SHIPPED)

    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true)

    await user.type(box(), "One more line.")
    await user.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => {
      expect(requests.find((one) => one.method === "PUT")?.body).toEqual({
        document: `${SHIPPED.document}One more line.`,
      })
    })
  })

  it("puts the edits back on Discard, without asking the daemon anything", async () => {
    const user = userEvent.setup()
    renderEditor(SHIPPED)

    await user.type(box(), "One more line.")
    await user.click(screen.getByRole("button", { name: "Discard" }))

    expect(box().value).toBe(SHIPPED.document)
    expect(requests).toEqual([])
  })

  it("shows what the daemon refused a save for", async () => {
    const user = userEvent.setup()
    stubDaemon(() => errorResponse(400, "invalid", "The frontmatter names no skill."))
    renderEditor(SHIPPED)

    await user.type(box(), "broken")
    await user.click(screen.getByRole("button", { name: "Save" }))

    expect((await screen.findByRole("alert")).textContent).toContain(
      "The frontmatter names no skill.",
    )
  })

  it("follows the row when the document changes under it", () => {
    const { rerender } = renderScreen(<SkillEditor skill={SHIPPED} onDeleted={vi.fn()} />)
    expect(box().value).toBe(SHIPPED.document)

    // A reset, or an edit that arrived on the event stream: the box must not
    // be left showing a text the daemon no longer holds.
    rerender(<SkillEditor skill={REWRITTEN} onDeleted={vi.fn()} />)
    expect(box().value).toBe(REWRITTEN.document)
  })
})

describe("a skill Ariadne ships", () => {
  it("is reset rather than deleted", () => {
    renderEditor(REWRITTEN)

    expect(screen.getByRole("button", { name: "Reset" })).toBeDefined()
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull()
  })

  it("has nothing to reset to while it is still on the shipped text", () => {
    renderEditor(SHIPPED)
    expect((screen.getByRole("button", { name: "Reset" }) as HTMLButtonElement).disabled).toBe(true)
  })

  it("asks before it throws the written text away", async () => {
    const user = userEvent.setup()
    renderEditor(REWRITTEN)

    await user.click(screen.getByRole("button", { name: "Reset" }))
    const dialog = await screen.findByRole("dialog", { name: "Reset coding?" })
    await user.click(within(dialog).getByRole("button", { name: "Reset" }))

    await waitFor(() => {
      expect(requests.some((one) => one.path === "/v1/skills/coding/document/reset")).toBe(true)
    })
  })
})

describe("a skill of your own", () => {
  it("is deleted rather than reset", () => {
    renderEditor(MINE)

    expect(screen.getByRole("button", { name: "Delete" })).toBeDefined()
    expect(screen.queryByRole("button", { name: "Reset" })).toBeNull()
  })

  it("asks before it goes, and tells the screen once it has", async () => {
    const user = userEvent.setup()
    stubDaemon(() => new Response(null, { status: 204 }))
    const { onDeleted } = renderEditor(MINE)

    await user.click(screen.getByRole("button", { name: "Delete" }))
    const dialog = await screen.findByRole("dialog", { name: "Delete api-design?" })
    await user.click(within(dialog).getByRole("button", { name: "Delete" }))

    await waitFor(() => expect(onDeleted).toHaveBeenCalled())
    expect(requests.some((one) => one.method === "DELETE")).toBe(true)
  })

  it("stays where it is when the daemon refuses, and says why", async () => {
    const user = userEvent.setup()
    stubDaemon(() => errorResponse(409, "conflict", "skill api-design is still loaded by 2 agents"))
    const { onDeleted } = renderEditor(MINE)

    await user.click(screen.getByRole("button", { name: "Delete" }))
    const dialog = await screen.findByRole("dialog", { name: "Delete api-design?" })
    await user.click(within(dialog).getByRole("button", { name: "Delete" }))

    expect((await screen.findByText(/still loaded by 2 agents/)).textContent).toBeDefined()
    expect(onDeleted).not.toHaveBeenCalled()
  })
})
