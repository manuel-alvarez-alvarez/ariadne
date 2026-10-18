import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language"
import { type Diagnostic, lintGutter, setDiagnostics } from "@codemirror/lint"
import { Compartment, EditorState, type Extension } from "@codemirror/state"
import { oneDarkHighlightStyle } from "@codemirror/theme-one-dark"
import { EditorView, lineNumbers } from "@codemirror/view"
import { useTheme } from "next-themes"
import { useEffect, useRef } from "react"

import { workflowLanguage } from "./workflow-language"

const workflowEditorTheme = EditorView.theme({
  "&": {
    height: "100%",
    border: "1px solid var(--input)",
    borderRadius: "0.375rem",
    fontSize: "12px",
  },
  "&.cm-focused": { outline: "2px solid var(--ring)", outlineOffset: "2px" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55", overflow: "auto" },
  ".cm-content": { minHeight: "12rem", padding: "0.5rem 0" },
  ".cm-line": { padding: "0 0.75rem" },
  ".cm-gutters": {
    backgroundColor: "var(--muted)",
    borderRight: "1px solid var(--border)",
    color: "var(--muted-foreground)",
  },
  ".cm-lintRange-error": {
    backgroundImage: "none",
    textDecoration: "underline wavy var(--destructive)",
  },
})
const noExtensions: Extension = []

function colorTheme(dark: boolean): Extension {
  return [
    EditorView.darkTheme.of(dark),
    syntaxHighlighting(dark ? oneDarkHighlightStyle : defaultHighlightStyle),
  ]
}

export function WorkflowCodeEditor({
  value,
  onChange,
  error,
  id,
  labelId,
  extensions = noExtensions,
}: {
  value: string
  onChange: (value: string) => void
  error: { line: number; message: string } | null
  id: string
  labelId: string
  extensions?: Extension
}) {
  const { resolvedTheme } = useTheme()
  const dark = resolvedTheme === "dark"
  const host = useRef<HTMLDivElement | null>(null)
  const view = useRef<EditorView | null>(null)
  const document = useRef(value)
  const onDocumentChange = useRef(onChange)
  const theme = useRef(new Compartment())
  const extraExtensions = useRef(new Compartment())
  const editorConfig = useRef({ dark, extensions, id, labelId })
  document.current = value
  onDocumentChange.current = onChange
  editorConfig.current = { dark, extensions, id, labelId }

  useEffect(() => {
    const parent = host.current
    if (!parent) return
    const editor = new EditorView({
      parent,
      state: EditorState.create({
        doc: document.current,
        extensions: [
          lineNumbers(),
          workflowLanguage,
          workflowEditorTheme,
          theme.current.of(colorTheme(editorConfig.current.dark)),
          lintGutter(),
          EditorView.contentAttributes.of({
            id: editorConfig.current.id,
            "aria-labelledby": editorConfig.current.labelId,
            spellcheck: "false",
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onDocumentChange.current(update.state.doc.toString())
          }),
          extraExtensions.current.of(editorConfig.current.extensions),
        ],
      }),
    })
    view.current = editor
    return () => {
      view.current = null
      editor.destroy()
    }
  }, [])

  useEffect(() => {
    view.current?.dispatch({ effects: theme.current.reconfigure(colorTheme(dark)) })
  }, [dark])

  useEffect(() => {
    view.current?.dispatch({ effects: extraExtensions.current.reconfigure(extensions) })
  }, [extensions])

  useEffect(() => {
    const editor = view.current
    if (!editor || editor.state.doc.toString() === value) return
    editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: value } })
  }, [value])

  useEffect(() => {
    const editor = view.current
    if (!editor) return
    const diagnostics: Diagnostic[] = error
      ? [
          {
            from: editor.state.doc.line(Math.min(Math.max(error.line, 1), editor.state.doc.lines))
              .from,
            to: editor.state.doc.line(Math.min(Math.max(error.line, 1), editor.state.doc.lines)).to,
            severity: "error",
            message: error.message,
          },
        ]
      : []
    editor.dispatch(setDiagnostics(editor.state, diagnostics))
  }, [error])

  return <div ref={host} className="min-h-48 flex-1" />
}
