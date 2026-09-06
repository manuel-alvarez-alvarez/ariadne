// @vitest-environment jsdom

/**
 * Writing a skill of your own: two fields, and what each side validates.
 *
 * The client checks the one thing it can — a skill needs a name — and the
 * daemon checks what it alone knows: a name already taken. The template is
 * the third thing here, because it is what stops the name being typed twice.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { aSkill } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { CreateSkillDialog } from "./create-skill-dialog"

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []

function stubDaemon(answer: () => Response) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const raw = await request.text()
    requests.push({
      method: request.method,
      path: new URL(request.url).pathname,
      body: raw.length > 0 ? JSON.parse(raw) : null,
    })
    return answer()
  })
}

function renderDialog() {
  const onCreated = vi.fn()
  const onOpenChange = vi.fn()
  renderScreen(<CreateSkillDialog open onOpenChange={onOpenChange} onCreated={onCreated} />)
  return { onCreated, onOpenChange }
}

function dialog(): HTMLElement {
  return screen.getByRole("dialog", { name: "New skill" })
}

function documentBox(): HTMLTextAreaElement {
  return within(dialog()).getByLabelText("Document") as HTMLTextAreaElement
}

beforeEach(() => {
  requests = []
  stubDaemon(() => jsonResponse(aSkill({ name: "api-design", builtin: false }), 201))
})

describe("the document", () => {
  it("opens on the frontmatter every skill starts with", () => {
    renderDialog()
    expect(documentBox().value).toContain("name: my-skill")
    expect(documentBox().value).toContain("description:")
  })

  it("follows the name until the document is touched, so nobody types it twice", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "api-design")

    expect(documentBox().value).toContain("name: api-design")
  })
})

describe("creating", () => {
  it("refuses a skill with no name, before the daemon is asked", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.click(within(dialog()).getByRole("button", { name: "Create skill" }))

    expect(await screen.findByText("A skill needs a name.")).toBeDefined()
    expect(requests).toEqual([])
  })

  it("sends the name and the whole document, and hands the new skill back", async () => {
    const user = userEvent.setup()
    const { onCreated, onOpenChange } = renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "  api-design  ")
    await user.click(within(dialog()).getByRole("button", { name: "Create skill" }))

    await waitFor(() => {
      // Trimmed: a name with a space around it is the same name.
      expect(requests.find((one) => one.method === "POST")?.body).toMatchObject({
        name: "api-design",
      })
    })
    expect(requests.find((one) => one.method === "POST")?.body?.document).toContain(
      "name: api-design",
    )
    expect(onOpenChange).toHaveBeenCalledWith(false)
    await waitFor(() => expect(onCreated).toHaveBeenCalled())
  })

  it("puts a name already taken on the name field", async () => {
    const user = userEvent.setup()
    stubDaemon(() => errorResponse(409, "conflict", "skill coding is taken"))
    const { onOpenChange } = renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "coding")
    await user.click(within(dialog()).getByRole("button", { name: "Create skill" }))

    expect(await screen.findByText('A skill named "coding" already exists.')).toBeDefined()
    // The dialog stays open on the name the user has to change.
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
  })

  it("shows any other refusal on the dialog itself", async () => {
    const user = userEvent.setup()
    stubDaemon(() => errorResponse(400, "invalid", "The frontmatter names no skill."))
    renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "api-design")
    await user.click(within(dialog()).getByRole("button", { name: "Create skill" }))

    expect(await screen.findByText(/The frontmatter names no skill\./)).toBeDefined()
  })
})
