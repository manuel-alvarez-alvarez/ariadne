/**
 * Where a stepped task is in its goal's workflow, as one strip: a segment per
 * column, in order. The column the task is in is highlighted, the ones behind
 * it are marked done, and the ones ahead are plain. Each segment's hint is the
 * column's description — what the agent there is asked to do.
 *
 * A task still pending is in no column yet, so nothing is highlighted; a
 * finished task has been through every one.
 */

import { CheckIcon } from "lucide-react"

import type { TaskDto, WorkflowStepDto } from "@/api"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

type SegmentState = "done" | "current" | "ahead"

const SEGMENT: Record<SegmentState, string> = {
  done: "bg-status-done-soft text-status-done-fg",
  current: "bg-status-active-soft text-status-active-fg font-medium ring-1 ring-status-active",
  ahead: "bg-muted text-muted-foreground",
}

export function StepStrip({ task, steps }: { task: TaskDto; steps: WorkflowStepDto[] }) {
  const current = steps.findIndex((step) => step.id === task.step)
  return (
    <ol aria-label="Workflow steps" className="flex gap-1">
      {steps.map((step, index) => {
        const state = segmentState(task, index, current)
        return (
          <li
            key={step.id}
            data-state={state}
            aria-current={state === "current" ? "step" : undefined}
            className="min-w-0 flex-1"
          >
            <Tooltip>
              <TooltipTrigger
                render={
                  <span
                    className={cn(
                      "flex items-center justify-center gap-1 rounded-md px-2 py-1 text-xs",
                      SEGMENT[state],
                    )}
                  />
                }
              >
                {state === "done" ? <CheckIcon className="size-3 shrink-0" aria-hidden /> : null}
                <span className="truncate">{step.title}</span>
              </TooltipTrigger>
              <TooltipContent>{step.description || step.title}</TooltipContent>
            </Tooltip>
          </li>
        )
      })}
    </ol>
  )
}

function segmentState(task: TaskDto, index: number, current: number): SegmentState {
  if (task.status === "finished") return "done"
  if (current < 0) return "ahead"
  if (index < current) return "done"
  return index === current ? "current" : "ahead"
}
