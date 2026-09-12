/**
 * The diff a tool call showed, as the console opens it: the file's new text
 * as the document, its old text as what the merge view compares it against.
 *
 * This is the task diff viewer's lighter sibling. That one is fed a unified
 * diff and has to carry both files' line numbers alongside a stitched
 * document; an ACP `diff` content entry carries the two texts whole, so the
 * merge view can compute the chunks itself and there is nothing to stitch and
 * nothing to number. The theme is the viewer's own, so a change reads the
 * same wherever it is shown — always in its dark mode here, since the console
 * is one dark surface in both of the app's themes.
 */

import { syntaxHighlighting } from "@codemirror/language"
import { unifiedMergeView } from "@codemirror/merge"
import { EditorState } from "@codemirror/state"
import { oneDarkHighlightStyle } from "@codemirror/theme-one-dark"
import { EditorView } from "@codemirror/view"
import { useEffect, useRef } from "react"

import { diffTheme, languageFor } from "@/features/tasks/diff-editor"

export function ConsoleDiff({
  path,
  oldText,
  newText,
}: {
  path: string
  /** `null` for a file the tool created. */
  oldText: string | null
  newText: string
}) {
  const host = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    const parent = host.current
    if (!parent) return
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: newText,
        extensions: [
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          EditorView.lineWrapping,
          diffTheme,
          EditorView.darkTheme.of(true),
          syntaxHighlighting(oneDarkHighlightStyle),
          ...languageFor(path),
          unifiedMergeView({ original: oldText ?? "", mergeControls: false, gutter: false }),
        ],
      }),
    })
    return () => {
      view.destroy()
    }
  }, [path, oldText, newText])

  return (
    <section aria-label={`Diff of ${path}`} className="overflow-hidden rounded-md border">
      <p className="border-b bg-muted/40 px-3 py-1 text-muted-foreground">{path}</p>
      <div ref={host} />
    </section>
  )
}
