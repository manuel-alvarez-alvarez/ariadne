// @vitest-environment jsdom

/**
 * The two line-number columns, which are the one thing about the viewer that a
 * reader can check against a checkout.
 *
 * The document CodeMirror shows is the file's hunks stitched together, so
 * neither column can be inferred from it: the new numbers are carried
 * alongside, the old ones with them, and the deleted lines — which are not in
 * the document at all — are numbered from the old side of the merge view's own
 * chunks. That last one is the case with nothing pure to assert, which is why
 * this renders the editor rather than testing `buildDiffDocuments` again.
 */

import { render } from "@testing-library/react"
import { expect, it } from "vitest"

import { DiffEditor } from "./diff-editor"
import { type DiffFile, parseUnifiedDiff } from "./diff-parse"

/** A hunk far enough down the file that its numbers cannot be an accident. */
const DIFF = `diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -10,4 +10,4 @@
 keep
-gone
-also gone
+added
+more
`

function fileOf(diff: string): DiffFile {
  const file = parseUnifiedDiff(diff).files[0]
  if (!file) throw new Error("the diff has no file")
  return file
}

/**
 * What one gutter column reads, top to bottom, a string per row. The spacer
 * that gives the column its width is hidden and is not one of them.
 */
function column(container: HTMLElement, which: "old" | "new"): string[] {
  const gutter = container.querySelector(`.cm-${which}LineNumbers`)
  if (!gutter) throw new Error(`no ${which} line-number column`)
  return [...gutter.querySelectorAll<HTMLElement>(".cm-gutterElement")]
    .filter((element) => element.style.visibility !== "hidden")
    .map((element) => element.textContent ?? "")
}

it("numbers both sides, and leaves the added lines out of the old one", () => {
  const { container } = render(<DiffEditor file={fileOf(DIFF)} wrap={false} />)

  // The old column has a row for the deleted chunk's widget as well as one per
  // line: the context line, the two deleted lines stacked in the one marker a
  // gutter may put beside a block widget, then the two added lines, which the
  // old file has no numbers for. The new column has only the document's own
  // lines — it puts nothing beside the widget — and is spaced against them.
  expect(column(container, "old")).toEqual(["10", "1112", "", ""])
  expect(column(container, "new")).toEqual(["10", "11", "12"])
})

it("numbers a file whose every line is added, which has no old side at all", () => {
  const added = `diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+one
+two
`
  const { container } = render(<DiffEditor file={fileOf(added)} wrap={false} />)

  expect(column(container, "old")).toEqual(["", ""])
  expect(column(container, "new")).toEqual(["1", "2"])
})
