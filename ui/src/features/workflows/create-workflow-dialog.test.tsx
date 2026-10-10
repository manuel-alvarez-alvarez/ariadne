// @vitest-environment jsdom

/**
 * Writing a workflow of your own: the same editor and preview the edit pane
 * gives an existing one, plus the one thing only a new workflow needs — the
 * template that follows the name until the document is touched.
 */

import { Transaction } from "@codemirror/state"
import { activateHover, EditorView } from "@codemirror/view"
import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest"

import { aWorkflow } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { CreateWorkflowDialog } from "./create-workflow-dialog"

const rect = () => new DOMRect(0, 0, 0, 0)
const rects = () =>
  Object.assign([rect()], { item: (index: number) => (index === 0 ? rect() : null) })
const range = Range.prototype as Range & {
  getClientRects: () => DOMRectList
  getBoundingClientRect: () => DOMRect
}
const clientRects = range.getClientRects
const boundingRect = range.getBoundingClientRect

beforeAll(() => {
  range.getClientRects = rects as () => DOMRectList
  range.getBoundingClientRect = rect
})
afterAll(() => {
  range.getClientRects = clientRects
  range.getBoundingClientRect = boundingRect
})

const PARSED_STEPS = [
  {
    id: "develop",
    title: "Develop",
    description: "Build the task on its branch and commit it.",
    skills: ["coding"],
    rank: "balanced" as const,
    gate: "committed",
  },
  {
    id: "review",
    title: "Review",
    description:
      "Run the whole suite and judge the change against the task and the repository rules.",
    skills: ["code-review"],
    rank: "frontier" as const,
    gate: null,
  },
  {
    id: "merge",
    title: "Merge",
    description: "Rebase onto the base branch, run the whole suite, squash, fast-forward and push.",
    skills: ["merge"],
    rank: "fast" as const,
    gate: "merged",
  },
]

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []

function stubDaemon(
  parse: (document: string) => Response = () =>
    jsonResponse({ name: "my-workflow", steps: PARSED_STEPS }),
  skills: { name: string; summary: string }[] = [],
) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })
    if (pathname === "/v1/skills" && request.method === "GET") return jsonResponse(skills)
    if (pathname === "/v1/workflows/parse" && request.method === "POST")
      return parse((body as { document: string }).document)
    if (pathname === "/v1/workflows" && request.method === "POST")
      return jsonResponse(aWorkflow({ name: (body as { name: string }).name }), 201)
    return new Response("not stubbed", { status: 404 })
  })
}

/** The real `EditorView` behind the dialog's "Document" textbox. */
function editorView(): EditorView {
  const content = screen.getByRole("textbox", { name: "Document" })
  const view = EditorView.findFromDOM(content.closest(".cm-editor") ?? content)
  if (!view) throw new Error("expected the document textbox to be a live CodeMirror editor")
  return view
}

function renderDialog() {
  const onCreated = vi.fn()
  const onOpenChange = vi.fn()
  renderScreen(<CreateWorkflowDialog open onOpenChange={onOpenChange} onCreated={onCreated} />)
  return { onCreated, onOpenChange }
}

function dialog(): HTMLElement {
  return screen.getByRole("dialog", { name: "New workflow" })
}

beforeEach(() => {
  requests = []
  stubDaemon()
})

describe("the editor and the preview", () => {
  it("puts the editor and the preview in two columns", async () => {
    renderDialog()
    await screen.findByRole("heading", { name: "Develop" })

    expect(screen.getByLabelText("Workflow editor").parentElement?.className).toContain(
      "grid-cols-2",
    )
    expect(screen.getByLabelText("Workflow preview").parentElement?.className).toContain(
      "grid-cols-2",
    )
  })

  it("sends the live document to the parser, and redraws the preview from its answer", async () => {
    stubDaemon((doc) =>
      jsonResponse({
        name: "my-workflow",
        steps: doc.includes("# a step of my own")
          ? [
              {
                id: "mine",
                title: "Mine",
                description: "A step of my own.",
                skills: [],
                rank: null,
                gate: null,
              },
            ]
          : PARSED_STEPS,
      }),
    )
    renderDialog()
    await screen.findByRole("heading", { name: "Develop" })

    const view = editorView()
    view.dispatch({
      changes: { from: view.state.doc.length, insert: "\n    # a step of my own" },
    })

    await waitFor(() => {
      const parse = requests.filter((one) => one.path === "/v1/workflows/parse").at(-1)
      expect(parse?.body?.document).toContain("# a step of my own")
    })
    expect(await screen.findByRole("heading", { name: "Mine" })).toBeDefined()
    expect(screen.queryByRole("heading", { name: "Develop" })).toBeNull()
    expect(screen.queryByRole("heading", { name: "Review" })).toBeNull()
    expect(screen.queryByRole("heading", { name: "Merge" })).toBeNull()
  })

  it("marks a parse error on its line in the editor, and shows it in the preview", async () => {
    stubDaemon(
      () =>
        new Response(
          JSON.stringify({
            error: { code: "workflow_invalid", message: "Unknown rank", details: { line: 2 } },
          }),
          { status: 400, headers: { "content-type": "application/json" } },
        ),
    )
    renderDialog()

    expect((await screen.findByRole("alert")).textContent).toContain("Line 2: Unknown rank")
    expect(document.querySelector(".cm-lintRange-error")).not.toBeNull()
  })

  it("completes a skill name from the catalog after skills: in the dialog editor", async () => {
    stubDaemon(undefined, [{ name: "my-skill", summary: "Does my own thing." }])
    renderDialog()
    await screen.findByRole("heading", { name: "Develop" })

    const view = editorView()
    let pos = view.state.doc.length
    view.dispatch({
      changes: { from: pos, insert: "\n    skills" },
      selection: { anchor: pos + "\n    skills".length },
    })
    pos = view.state.doc.length
    view.dispatch({
      changes: { from: pos, insert: ":" },
      selection: { anchor: pos + 1 },
      annotations: Transaction.userEvent.of("input.type"),
    })

    const option = await screen.findByRole("option", { name: /my-skill/ })
    expect(within(option).getByText("Does my own thing.")).toBeDefined()
  })

  it("explains a skill named on a skills: line, through the dialog editor's hover help", async () => {
    stubDaemon(undefined, [{ name: "my-skill", summary: "Does my own thing." }])
    renderDialog()
    await screen.findByRole("heading", { name: "Develop" })

    const view = editorView()
    view.dispatch({ changes: { from: view.state.doc.length, insert: "\n    skills: my-skill" } })
    const pos = view.state.doc.toString().lastIndexOf("my-skill") + 2
    activateHover(view, pos, 1)

    await waitFor(() => {
      expect(document.querySelector(".cm-tooltip-workflow-help")?.textContent).toBe(
        "Does my own thing.",
      )
    })
  })
})

describe("the document", () => {
  it("opens on the template named after the placeholder", () => {
    renderDialog()

    expect(screen.getByRole("textbox", { name: "Document" }).textContent).toContain(
      "workflow my-workflow",
    )
  })

  it("follows the name until the document is touched, so nobody types it twice", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "release-flow")

    expect(screen.getByRole("textbox", { name: "Document" }).textContent).toContain(
      "workflow release-flow",
    )
  })

  it("stops following the name once the document itself is edited", async () => {
    const user = userEvent.setup()
    renderDialog()

    const editor = screen.getByRole("textbox", { name: "Document" })
    await user.click(editor)
    await user.paste("\n# mine")
    await user.type(within(dialog()).getByLabelText("Name"), "release-flow")

    expect(editor.textContent).not.toContain("workflow release-flow")
    expect(editor.textContent).toContain("workflow my-workflow")
  })
})

describe("creating", () => {
  it("refuses a workflow with no name, before the daemon is asked", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.click(within(dialog()).getByRole("button", { name: "Create workflow" }))

    expect(await screen.findByText("A workflow needs a name.")).toBeDefined()
    expect(requests.some((one) => one.method === "POST" && one.path === "/v1/workflows")).toBe(
      false,
    )
  })

  it("sends the name and the live document, and hands the new workflow back", async () => {
    const user = userEvent.setup()
    const { onCreated, onOpenChange } = renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "  release-flow  ")
    await user.click(within(dialog()).getByRole("button", { name: "Create workflow" }))

    await waitFor(() => {
      const created = requests.find((one) => one.method === "POST" && one.path === "/v1/workflows")
      expect(created?.body).toMatchObject({ name: "release-flow" })
    })
    const created = requests.find((one) => one.method === "POST" && one.path === "/v1/workflows")
    expect(created?.body?.document).toContain("workflow release-flow")
    expect(onOpenChange).toHaveBeenCalledWith(false)
    await waitFor(() => expect(onCreated).toHaveBeenCalled())
  })

  it("puts a name already taken on the name field", async () => {
    const user = userEvent.setup()
    stubDaemon()
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const { pathname } = new URL(request.url)
      if (pathname === "/v1/skills" && request.method === "GET") return jsonResponse([])
      if (pathname === "/v1/workflows/parse" && request.method === "POST")
        return jsonResponse({ name: "my-workflow", steps: PARSED_STEPS })
      if (pathname === "/v1/workflows" && request.method === "POST")
        return errorResponse(409, "conflict", "workflow develop-review-merge is taken")
      return new Response("not stubbed", { status: 404 })
    })
    const { onOpenChange } = renderDialog()

    await user.type(within(dialog()).getByLabelText("Name"), "develop-review-merge")
    await user.click(within(dialog()).getByRole("button", { name: "Create workflow" }))

    expect(
      await screen.findByText('A workflow named "develop-review-merge" already exists.'),
    ).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
  })
})

describe("closing with unsaved work", () => {
  it("asks before dropping a document edited by hand", async () => {
    const user = userEvent.setup()
    renderDialog()

    const editor = screen.getByRole("textbox", { name: "Document" })
    await user.click(editor)
    await user.paste("\n# mine")
    await user.keyboard("{Escape}")

    expect(await screen.findByRole("dialog", { name: "Discard changes?" })).toBeDefined()
  })
})
