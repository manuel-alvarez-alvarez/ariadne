/**
 * The group rule of the turn console, as the terminal console holds it (008,
 * rule 59): consecutive tool calls of kind `read` or `search` in one turn
 * fold into one group. A step of another kind ends the group, and a group of
 * one step is a step.
 */

import type { StepDto } from "@/api"

export type ToolStep = Extract<StepDto, { type: "tool_call" }>

/** A row of a turn: one step, or a group of two or more read and search calls. */
type Entry = { type: "step"; step: StepDto } | { type: "group"; steps: ToolStep[] }

function folds(step: StepDto): step is ToolStep {
  return step.type === "tool_call" && (step.kind === "read" || step.kind === "search")
}

/** The rows of the steps shown, in their order, with each run of read and search calls folded. */
export function groupSteps(steps: readonly StepDto[]): Entry[] {
  const entries: Entry[] = []
  let run: ToolStep[] = []
  const flush = () => {
    const [only] = run
    if (run.length === 1 && only) entries.push({ type: "step", step: only })
    else if (run.length > 1) entries.push({ type: "group", steps: run })
    run = []
  }
  for (const step of steps) {
    if (folds(step)) run.push(step)
    else {
      flush()
      entries.push({ type: "step", step })
    }
  }
  flush()
  return entries
}

/** Running while a step runs, else failed if a step failed, else completed. */
export function groupState(steps: readonly ToolStep[]): "running" | "failed" | "completed" {
  if (steps.some((step) => step.status === "running")) return "running"
  if (steps.some((step) => step.status === "failed")) return "failed"
  return "completed"
}
