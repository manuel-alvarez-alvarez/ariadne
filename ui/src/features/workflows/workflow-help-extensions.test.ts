// @vitest-environment jsdom

/**
 * The autocomplete extension wired into the real CodeMirror pipeline, proving
 * that what `workflow-help.ts` computes actually reaches `acceptCompletion`:
 * a pure-function `to` that is right but never forwarded by the adapter
 * would pass `workflow-help.test.ts` and still corrupt the document here.
 */

import { acceptCompletion, completionStatus, startCompletion } from "@codemirror/autocomplete"
import { EditorState } from "@codemirror/state"
import { EditorView } from "@codemirror/view"
import { waitFor } from "@testing-library/react"
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest"

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

let view: EditorView | undefined
afterEach(() => {
  view?.destroy()
  view = undefined
  document.body.innerHTML = ""
})

/** A live editor on `text`, cursor placed at `cursor`, with the real autocomplete wired in. */
function openAt(text: string, cursor: number): EditorView {
  view = new EditorView({
    state: EditorState.create({
      doc: text,
      selection: { anchor: cursor },
      extensions: [workflowAutocomplete([])],
    }),
    parent: document.body,
  })
  return view
}

/**
 * CodeMirror refuses `acceptCompletion` within its own 75ms anti-click-through
 * guard (`interactionDelay`, measured from when the dropdown opened). The
 * editor under test never races a real user, so this just lets that much
 * real time pass before accepting.
 */
async function waitPastInteractionDelay(): Promise<void> {
  const openedAt = Date.now()
  await waitFor(() => expect(Date.now() - openedAt).toBeGreaterThan(300))
}

describe("workflowAutocomplete, applied through CodeMirror", () => {
  it("replaces only the rank token the cursor sits inside, keeping the rest of the band", async () => {
    const text = "workflow develop-review\n    ranks: balanced, frontier, local"
    const cursor = text.indexOf("frontier") + 2 // "fr|ontier"
    const editor = openAt(text, cursor)

    startCompletion(editor)
    await waitFor(() => expect(completionStatus(editor.state)).toBe("active"))
    await waitPastInteractionDelay()
    expect(acceptCompletion(editor)).toBe(true)

    expect(editor.state.doc.toString()).toBe(text)
  })
})
