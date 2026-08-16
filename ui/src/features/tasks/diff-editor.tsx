/**
 * One file of a diff, in a read-only CodeMirror 6 view.
 *
 * The daemon hands over a unified diff, not the two files, so the documents
 * fed to `@codemirror/merge` are the file's hunks stitched together: the new
 * side is the editor's document, the old side is what the merge view compares
 * it against, and the deleted lines come back as its widgets. That buys real
 * syntax highlighting of the code — the alternative, colouring the raw `+`/`-`
 * text, cannot be highlighted at all — at the cost of the line numbers, which
 * the stitched document no longer implies and which are therefore carried
 * alongside it and fed to the gutter.
 *
 * Everything is styled off the app's CSS variables so the viewer follows the
 * theme — the add/remove tints included, which is why nothing here spells a
 * colour out per mode.
 */

import { javascript } from "@codemirror/lang-javascript"
import { json } from "@codemirror/lang-json"
import { markdown } from "@codemirror/lang-markdown"
import { rust } from "@codemirror/lang-rust"
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language"
import { unifiedMergeView } from "@codemirror/merge"
import { EditorState, type Extension, RangeSetBuilder } from "@codemirror/state"
import { oneDarkHighlightStyle } from "@codemirror/theme-one-dark"
import {
  Decoration,
  type DecorationSet,
  EditorView,
  lineNumbers,
  WidgetType,
} from "@codemirror/view"
import { useTheme } from "next-themes"
import { useEffect, useMemo, useRef } from "react"

import { buildDiffDocuments, type DiffDocuments, type DiffFile } from "./diff-parse"

/** Beyond this many lines a single file is left to the raw view. */
export const LARGE_FILE_LINES = 3000

const LANGUAGES: Record<string, () => Extension> = {
  cjs: () => javascript(),
  js: () => javascript(),
  json: () => json(),
  jsonc: () => json(),
  jsx: () => javascript({ jsx: true }),
  markdown: () => markdown(),
  md: () => markdown(),
  mjs: () => javascript(),
  mts: () => javascript({ typescript: true }),
  rs: () => rust(),
  ts: () => javascript({ typescript: true }),
  tsx: () => javascript({ typescript: true, jsx: true }),
}

function languageFor(path: string): Extension[] {
  const extension = path.split(".").pop()?.toLowerCase() ?? ""
  const language = LANGUAGES[extension]
  return language ? [language()] : []
}

/** The `@@ -1,4 +1,5 @@` line, kept out of the document and shown above its hunk. */
class HunkHeaderWidget extends WidgetType {
  readonly header: string
  readonly heading: string

  constructor(header: string, heading: string) {
    super()
    this.header = header
    this.heading = heading
  }

  override eq(other: HunkHeaderWidget): boolean {
    return other.header === this.header && other.heading === this.heading
  }

  override toDOM(): HTMLElement {
    const element = document.createElement("div")
    element.className = "cm-hunkHeader"
    const range = document.createElement("span")
    range.className = "cm-hunkRange"
    range.textContent = this.header.replace(/ @@.*$/, " @@")
    element.appendChild(range)
    if (this.heading) {
      const heading = document.createElement("span")
      heading.className = "cm-hunkHeading"
      heading.textContent = this.heading
      element.appendChild(heading)
    }
    return element
  }
}

/**
 * A block widget above the first line of every hunk. The offsets are computed
 * from the document text rather than from an `EditorState`, so the decorations
 * can be part of the state the view is created with.
 */
function hunkHeaderDecorations(docs: DiffDocuments): DecorationSet {
  const lengths = docs.doc.length === 0 ? [] : docs.doc.split("\n").map((line) => line.length)
  const offsets: number[] = []
  let offset = 0
  for (const length of lengths) {
    offsets.push(offset)
    offset += length + 1
  }

  const builder = new RangeSetBuilder<Decoration>()
  for (const hunk of docs.hunkStarts) {
    const at = offsets[hunk.line] ?? docs.doc.length
    builder.add(
      at,
      at,
      Decoration.widget({
        widget: new HunkHeaderWidget(hunk.header, hunk.heading),
        block: true,
        side: -1,
      }),
    )
  }
  return builder.finish()
}

/**
 * A file with nothing on the old side is every line added. Saying that with a
 * line decoration rather than a merge view avoids the empty deleted chunk the
 * merge view would otherwise draw above the first line.
 */
function addedLineDecorations(docs: DiffDocuments): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>()
  let offset = 0
  for (const line of docs.doc.split("\n")) {
    builder.add(offset, offset, Decoration.line({ class: "cm-addedLine" }))
    offset += line.length + 1
  }
  return builder.finish()
}

/**
 * The whole viewer's styling. Both themes share it: every colour is a token
 * that already resolves to the right value for the mode the app is in, so the
 * only thing left for the theme to say is which mode that is — `darkTheme`
 * below, which is what CodeMirror's own defaults key off.
 */
const baseTheme = EditorView.theme({
  "&": {
    backgroundColor: "transparent",
    color: "var(--foreground)",
    fontSize: "12px",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--font-mono)",
    lineHeight: "1.55",
  },
  ".cm-content": { padding: "0" },
  ".cm-line": { padding: "0 0.75rem" },
  ".cm-gutters": {
    backgroundColor: "transparent",
    borderRight: "1px solid var(--border)",
    color: "color-mix(in oklab, var(--muted-foreground) 80%, transparent)",
    userSelect: "none",
  },
  ".cm-lineNumbers .cm-gutterElement": { padding: "0 0.5rem 0 0.75rem", minWidth: "3ch" },
  ".cm-cursor, .cm-dropCursor": { display: "none" },
  ".cm-hunkHeader": {
    display: "flex",
    gap: "0.75rem",
    padding: "0.25rem 0.75rem",
    borderTop: "1px solid var(--border)",
    backgroundColor: "color-mix(in oklab, var(--muted) 60%, transparent)",
    color: "var(--muted-foreground)",
    fontSize: "11px",
  },
  ".cm-hunkHeader:first-child": { borderTop: "none" },
  ".cm-hunkHeading": { opacity: "0.75" },
  // The merge extension's accept/reject buttons have nothing to act on here.
  ".cm-deletedChunk .cm-chunkButtons": { display: "none" },
  "&.cm-merge-b .cm-changedLine, .cm-inlineChangedLine, .cm-addedLine": {
    backgroundColor: "var(--diff-add-soft)",
  },
  "&.cm-merge-b .cm-changedText": { background: "var(--diff-add-strong)" },
  ".cm-deletedChunk": { backgroundColor: "var(--diff-remove-soft)", paddingLeft: "0" },
  ".cm-deletedChunk .cm-deletedLine": { padding: "0 0.75rem" },
  ".cm-deletedChunk .cm-deletedText": { background: "var(--diff-remove-strong)" },
  ".cm-changeGutter": { display: "none" },
})

export function DiffEditor({ file, wrap }: { file: DiffFile; wrap: boolean }) {
  const { resolvedTheme } = useTheme()
  const dark = resolvedTheme === "dark"
  const host = useRef<HTMLDivElement | null>(null)
  const docs = useMemo(() => buildDiffDocuments(file), [file])
  const path = file.newPath ?? file.oldPath ?? ""
  // Nothing to compare against: the file is new (or its whole content is one
  // added hunk), so there is no old side for the merge view to diff.
  const allAdded = docs.original.length === 0 && docs.doc.length > 0

  useEffect(() => {
    const parent = host.current
    if (!parent) return

    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: docs.doc,
        extensions: [
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          // Unwrapped, a long line scrolls the editor sideways instead of
          // folding — which is the point of the toggle on a wide diff.
          ...(wrap ? [EditorView.lineWrapping] : []),
          lineNumbers({ formatNumber: (line) => String(docs.lineNumbers[line - 1] ?? "") }),
          EditorView.decorations.of(hunkHeaderDecorations(docs)),
          baseTheme,
          EditorView.darkTheme.of(dark),
          syntaxHighlighting(dark ? oneDarkHighlightStyle : defaultHighlightStyle),
          ...languageFor(path),
          allAdded
            ? EditorView.decorations.of(addedLineDecorations(docs))
            : unifiedMergeView({
                original: docs.original,
                mergeControls: false,
                gutter: false,
              }),
        ],
      }),
    })
    return () => {
      view.destroy()
    }
  }, [docs, dark, path, allAdded, wrap])

  return <div ref={host} />
}
