/**
 * A task's workflow steps as one compact row of dots, for the board card: a
 * check for a step the task has been through, a filled dot for the one it is
 * in, a ring for one still ahead. Each dot has a tooltip with what the step is
 * and who staffs it; the task panel's own track (`workflow-steps.tsx`) says
 * the same at full size, and shares its step states.
 *
 * The dots sit inside the card's link, so none of them is a tab stop
 * ({@link NOT_A_STOP}): the card's hidden hints line names the current step
 * for a keyboard.
 */

import { CheckIcon } from "lucide-react"

import type { TaskDto, WorkflowStepDto } from "@/api"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"
import { SessionLink } from "./task-sessions"
import { type StepState, stepState } from "./workflow-steps"

const NOT_A_STOP = -1

const DOT: Record<StepState, string> = {
  done: "border-status-done bg-status-done text-background",
  current: "border-status-active bg-status-active",
  ahead: "border-border bg-background",
}

export function StepTrack({ task, steps }: { task: TaskDto; steps: WorkflowStepDto[] }) {
  const current = steps.findIndex((step) => step.id === task.step)
  return (
    <ol
      aria-label="Workflow steps"
      className="flex min-w-0 max-w-full flex-wrap items-center gap-1"
    >
      {steps.map((step, index) => {
        const state = stepState(task, index, current)
        const agent = task.agents.find((candidate) => candidate.step === step.id)
        return (
          <li
            key={step.id}
            aria-current={state === "current" ? "step" : undefined}
            data-step-state={state}
            className="flex"
          >
            <Tooltip>
              <TooltipTrigger
                tabIndex={NOT_A_STOP}
                aria-label={step.title}
                render={
                  <span
                    className={cn(
                      "flex size-3 shrink-0 items-center justify-center rounded-full border",
                      DOT[state],
                    )}
                  />
                }
              >
                {state === "done" ? <CheckIcon className="size-2" strokeWidth={4} /> : null}
              </TooltipTrigger>
              <TooltipContent className="max-w-64 flex-col items-start gap-0.5">
                <span className="font-medium">{step.title}</span>
                {step.description ? <span>{step.description}</span> : null}
                <AgentLines agent={agent} />
              </TooltipContent>
            </Tooltip>
          </li>
        )
      })}
    </ol>
  )
}

type TaskAgent = TaskDto["agents"][number]

function AgentLines({ agent }: { agent: TaskAgent | undefined }) {
  if (!agent) return <span className="italic">none staffed</span>
  return (
    <>
      <span>
        {agent.model}
        {agent.effort ? ` @ ${agent.effort}` : ""}
      </span>
      <span>{agent.skills.length > 0 ? agent.skills.join(", ") : "no skills"}</span>
      {agent.session_id ? <SessionLink sessionId={agent.session_id} /> : null}
    </>
  )
}
