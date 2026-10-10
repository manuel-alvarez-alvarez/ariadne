import {
  autocompletion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete"
import type { Extension } from "@codemirror/state"
import { type EditorView, hoverTooltip, type Tooltip } from "@codemirror/view"

import { type SkillOption, workflowCompletionsAt, workflowHoverAt } from "./workflow-help"

function completionSource(skills: SkillOption[]) {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos)
    const cursor = context.pos - line.from
    const match = workflowCompletionsAt(line.text, cursor, line.number === 1, skills)
    if (!match) return null
    return {
      from: line.from + match.from,
      options: match.options.map((option) => ({
        label: option.label,
        detail: option.detail,
      })),
    }
  }
}

function hoverSource(skills: SkillOption[]) {
  return (view: EditorView, pos: number): Tooltip | null => {
    const line = view.state.doc.lineAt(pos)
    const match = workflowHoverAt(line.text, pos - line.from, line.number === 1, skills)
    if (!match) return null
    return {
      pos: line.from + match.from,
      end: line.from + match.to,
      above: true,
      create() {
        const dom = document.createElement("div")
        dom.className = "cm-tooltip-workflow-help"
        dom.textContent = match.text
        return { dom }
      },
    }
  }
}

/** Autocomplete for a workflow document's keys, rank and gate words, and skill names. */
export function workflowAutocomplete(skills: SkillOption[]): Extension {
  return autocompletion({ override: [completionSource(skills)] })
}

/** Hover help for a workflow document's keys, rank and gate words, and skill names. */
export function workflowHover(skills: SkillOption[]): Extension {
  return hoverTooltip(hoverSource(skills))
}
