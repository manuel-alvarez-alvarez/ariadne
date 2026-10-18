/**
 * Where a stepped task is in its goal's workflow, and who is running each
 * column, as one track: a node per column in order, and under each node the
 * column's title, its agent's skills, the model it runs on and a link to its
 * live session. Nothing opens and nothing is picked — every column says what
 * it is at once, and the section is a few lines tall however many columns
 * the workflow has. The column's description is left to the workflow's own
 * document.
 *
 * A node is its state: a filled check for a column the task has been through,
 * a pulsing dot for the one it is in, a hollow ring for one still ahead — and
 * the track between two nodes is drawn in the completion colour up to the
 * current one. The state is also a label read to assistive technology, so
 * the colour never stands alone.
 *
 * A task still pending is in no column yet, so nothing is current; a
 * finished task has been through every one.
 */

import { CheckIcon } from "lucide-react"

import type { TaskDto, WorkflowStepDto } from "@/api"
import { AgentSummary } from "@/features/models/agent-summary"
import { cn } from "@/lib/format"
import { SessionLink } from "./task-sessions"

type StepState = "done" | "current" | "ahead"

const STATE_META: Record<
  StepState,
  {
    label: string
    /** The node on the track. */
    node: string
    /** The track leaving the node, towards the next one. */
    track: string
  }
> = {
  done: {
    label: "Done",
    node: "border-status-done bg-status-done text-background",
    track: "bg-status-done",
  },
  current: {
    label: "Current",
    node: "border-status-active bg-status-active-soft",
    track: "bg-border",
  },
  ahead: {
    label: "Pending",
    node: "border-border bg-background",
    track: "bg-border",
  },
}

export function WorkflowSteps({ task, steps }: { task: TaskDto; steps: WorkflowStepDto[] }) {
  const current = steps.findIndex((step) => step.id === task.step)
  return (
    <ol aria-label="Workflow steps" className="flex items-start text-xs">
      {steps.map((step, index) => {
        const state = stepState(task, index, current)
        const meta = STATE_META[state]
        const agent = task.agents.find((candidate) => candidate.step === step.id)
        return (
          <li
            key={step.id}
            aria-current={state === "current" ? "step" : undefined}
            data-step-state={state}
            className="flex min-w-0 flex-1 flex-col items-center gap-1 text-center"
          >
            <span className="flex w-full items-center">
              <span
                className={cn(
                  "h-px flex-1",
                  index === 0 ? "invisible" : STATE_META[stepState(task, index - 1, current)].track,
                )}
                aria-hidden
              />
              <span
                className={cn(
                  "flex size-5 shrink-0 items-center justify-center rounded-full border-2",
                  meta.node,
                )}
                aria-hidden
              >
                {state === "done" ? (
                  <CheckIcon className="size-3" strokeWidth={3} />
                ) : state === "current" ? (
                  <span className="size-2 animate-pulse rounded-full bg-status-active" />
                ) : null}
              </span>
              <span
                className={cn("h-px flex-1", index === steps.length - 1 ? "invisible" : meta.track)}
                aria-hidden
              />
            </span>
            <span
              className={cn(
                "max-w-full truncate px-2",
                state === "current" ? "font-medium" : "text-muted-foreground",
              )}
            >
              {step.title}
            </span>
            <span className="sr-only">{meta.label}</span>
            {agent ? (
              <span className="flex w-full min-w-0 flex-col items-center gap-0.5 px-2">
                <AgentSummary
                  skills={agent.skills}
                  model={agent.model}
                  effort={agent.effort}
                  className="max-w-full items-center"
                />
                {agent.session_id ? (
                  <span className="whitespace-nowrap">
                    <SessionLink sessionId={agent.session_id} />
                  </span>
                ) : null}
              </span>
            ) : (
              <span className="text-muted-foreground italic">none staffed</span>
            )}
          </li>
        )
      })}
    </ol>
  )
}

function stepState(task: TaskDto, index: number, current: number): StepState {
  if (task.status === "finished") return "done"
  if (current < 0) return "ahead"
  if (index < current) return "done"
  return index === current ? "current" : "ahead"
}
