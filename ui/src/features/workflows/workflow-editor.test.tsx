// @vitest-environment jsdom

import {
  acceptCompletion,
  autocompletion,
  completionStatus,
  startCompletion,
} from "@codemirror/autocomplete"
import { EditorState } from "@codemirror/state"
import { EditorView } from "@codemirror/view"
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest"

import { aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { WorkflowEditor } from "./workflow-editor"
import { workflowAutocomplete } from "./workflow-help-extensions"

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

describe("the workflow editor", () => {
  it("puts the editor and preview in equal columns", () => {
    const workflow = aWorkflow()
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ name: workflow.name, steps: workflow.steps }),
    )
    renderScreen(<WorkflowEditor workflow={workflow} onDeleted={vi.fn()} />)

    expect(screen.getByLabelText("Workflow editor").parentElement?.className).toContain(
      "grid-cols-2",
    )
  })

  it("keeps a dirty draft when a newer row arrives", async () => {
    const user = userEvent.setup()
    const workflow = aWorkflow()
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ name: workflow.name, steps: workflow.steps }),
    )
    const { rerender } = renderScreen(<WorkflowEditor workflow={workflow} onDeleted={vi.fn()} />)
    const document = screen.getByRole("textbox", { name: "Document" })
    await user.click(document)
    await user.paste("\nMine")
    rerender(
      <WorkflowEditor
        workflow={{ ...workflow, document: `${workflow.document}\nElsewhere` }}
        onDeleted={vi.fn()}
      />,
    )

    expect(screen.getByRole("textbox", { name: "Document" }).textContent).toContain("Mine")
    expect(screen.getByText("This workflow changed elsewhere")).toBeDefined()
  })

  it("offers Reset for a shipped workflow and Delete for a user workflow", () => {
    const workflow = aWorkflow()
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ name: workflow.name, steps: workflow.steps }),
    )
    const { rerender } = renderScreen(<WorkflowEditor workflow={workflow} onDeleted={vi.fn()} />)
    expect(screen.getByRole("button", { name: "Reset" })).toBeDefined()
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull()
    rerender(<WorkflowEditor workflow={{ ...workflow, builtin: false }} onDeleted={vi.fn()} />)
    expect(screen.getByRole("button", { name: "Delete" })).toBeDefined()
  })

  it("parses each draft once for the editor and preview", async () => {
    const workflow = aWorkflow()
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ name: workflow.name, steps: workflow.steps }),
    )
    renderScreen(<WorkflowEditor workflow={workflow} onDeleted={vi.fn()} />)

    await screen.findByRole("heading", { name: "Develop" })
    // One parse call, plus the skill catalog the completions and hover help read.
    expect(daemonFetch).toHaveBeenCalledTimes(2)
  })

  it("marks the line the parser refuses in the editor", async () => {
    daemonFetch.mockImplementation(
      async () =>
        new Response(
          JSON.stringify({
            error: { code: "workflow_invalid", message: "Unknown rank", details: { line: 2 } },
          }),
          { status: 400, headers: { "content-type": "application/json" } },
        ),
    )
    renderScreen(<WorkflowEditor workflow={aWorkflow()} onDeleted={vi.fn()} />)

    expect((await screen.findByRole("alert")).textContent).toContain("Line 2: Unknown rank")
    expect(document.querySelector(".cm-lintRange-error")).not.toBeNull()
  })
})

/**
 * The autocomplete extension on a bare CodeMirror editor, proving that what
 * `workflow-help.ts` computes actually reaches `acceptCompletion`: a
 * pure-function `to` that is right but never forwarded by the adapter would
 * pass `workflow-help.test.ts` and still corrupt the document here.
 */
describe("workflowAutocomplete, applied through CodeMirror", () => {
  let view: EditorView | undefined
  afterEach(() => {
    view?.destroy()
    view = undefined
    document.body.innerHTML = ""
  })

  it("replaces only the rank token the cursor sits inside, keeping the rest of the band", async () => {
    const text = "workflow develop-review\n    ranks: balanced, frontier, local"
    const cursor = text.indexOf("frontier") + 2 // "fr|ontier"
    // `interactionDelay` (CodeMirror's own anti-click-through guard on
    // `acceptCompletion`) is irrelevant here; the config facet combines
    // across extensions, so it is turned off rather than waited out.
    const editor = new EditorView({
      state: EditorState.create({
        doc: text,
        selection: { anchor: cursor },
        extensions: [workflowAutocomplete([]), autocompletion({ interactionDelay: 0 })],
      }),
      parent: document.body,
    })
    view = editor

    startCompletion(editor)
    await waitFor(() => expect(completionStatus(editor.state)).toBe("active"))
    expect(acceptCompletion(editor)).toBe(true)

    expect(editor.state.doc.toString()).toBe(text)
  })
})
