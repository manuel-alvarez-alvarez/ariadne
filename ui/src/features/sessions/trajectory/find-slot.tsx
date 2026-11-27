/**
 * The find control above the turns. It searches what the console has
 * loaded: the prompt and the answer of each turn shown, and every step it
 * holds for them, live, in a loaded page or in the turn's attention. Find
 * leaves the folds, the place and the draft as they are. A search on the
 * daemon is not part of it: the line under the results says when older
 * turns are not loaded yet, and loads them on request.
 */

import type { StepDto, TurnDto } from "@/api"

import { TranscriptFind } from "../console/transcript-find"
import { contentText, foldSteps, type StepPage } from "./store"

export function TrajectoryFind({
  turns,
  pages,
  older,
  loading,
  loadOlder,
}: {
  turns: readonly TurnDto[]
  pages: Readonly<Record<string, StepPage>>
  older: boolean
  loading: boolean
  loadOlder: () => void
}) {
  return (
    <TranscriptFind
      entries={findEntries(turns, pages)}
      older={older}
      loading={loading}
      loadOlder={loadOlder}
    />
  )
}

/** The text of each loaded turn and step, in the turns' order, each step once. */
function findEntries(
  turns: readonly TurnDto[],
  pages: Readonly<Record<string, StepPage>>,
): { id: string; text: string }[] {
  return turns.flatMap((turn) => {
    const entries: { id: string; text: string }[] = []
    if (turn.prompt)
      entries.push({ id: `${turn.id}:prompt`, text: contentText(turn.prompt.content) })
    const seen = new Set<string>()
    for (const step of [...foldSteps(turn, pages[turn.id]).steps, ...turn.attention]) {
      if (seen.has(step.id)) continue
      seen.add(step.id)
      entries.push({ id: step.id, text: stepText(step) })
    }
    const answer = contentText(turn.answer)
    if (answer) entries.push({ id: `${turn.id}:answer`, text: answer })
    return entries.filter((entry) => entry.text.length > 0)
  })
}

/** What a reader sees of a step, as one text. */
function stepText(step: StepDto): string {
  switch (step.type) {
    case "message":
    case "thought":
      return contentText(step.content)
    case "tool_call":
      return [step.title, step.subject, step.preview].filter(Boolean).join("\n")
    case "permission":
      return [step.tool.title, step.tool.subject, step.question, step.answer?.label]
        .filter(Boolean)
        .join("\n")
    case "question":
      return [step.hint, step.question, step.answer].filter(Boolean).join("\n")
    case "note":
      return step.text
    case "error":
      return [step.text, step.diagnosis].filter(Boolean).join("\n")
    case "subagent":
      return [step.title, step.task].filter(Boolean).join("\n")
    case "compaction":
      return [contentText(step.summary), step.error].filter(Boolean).join("\n")
  }
}
