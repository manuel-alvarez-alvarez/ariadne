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

import { CheckIcon, ChevronRightIcon } from "lucide-react"

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

/** A workflow longer than this folds its steps far from the current one when the card is narrow. */
const FOLD_OVER = 5
/** How many steps either side of the current one stay drawn when it folds. */
const NEAR = 1

export function StepTrack({ task, steps }: { task: TaskDto; steps: WorkflowStepDto[] }) {
  const current = steps.findIndex((step) => step.id === task.step)
  const folds = steps.length > FOLD_OVER
  const anchor = current < 0 ? 0 : current
  const far = (index: number) => folds && Math.abs(index - anchor) > NEAR
  return (
    <ol
      aria-label="Workflow steps"
      className="flex min-w-0 max-w-full flex-nowrap items-center gap-1 overflow-hidden whitespace-nowrap"
    >
      {steps.map((step, index) => {
        const state = stepState(task, index, current)
        const agent = task.agents.find((candidate) => candidate.step === step.id)
        const first = index === 0
        return (
          <li
            key={step.id}
            aria-current={state === "current" ? "step" : undefined}
            data-step-state={state}
            className={cn(
              "min-w-0 items-center gap-1",
              state === "current" ? "flex shrink" : "flex shrink-[2]",
              far(index) && "hidden @[26rem]:flex",
            )}
          >
            {!first && <Arrow />}
            <Tooltip>
              <TooltipTrigger
                tabIndex={NOT_A_STOP}
                aria-label={step.title}
                render={
                  <span
                    className={cn(
                      "flex size-3.5 shrink-0 items-center justify-center rounded-full border",
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
            <span
              className={cn(
                "min-w-0 truncate",
                state === "current"
                  ? "font-medium text-foreground"
                  : "hidden max-w-24 @[22rem]:inline",
              )}
            >
              {step.title}
            </span>
          </li>
        )
      })}
      {folds && (
        <li role="presentation" className="shrink-0 @[26rem]:hidden">
          <Tooltip>
            <TooltipTrigger
              tabIndex={NOT_A_STOP}
              aria-label={`${steps.filter((_, index) => far(index)).length} more steps`}
              render={<span />}
            >
              …
            </TooltipTrigger>
            <TooltipContent className="max-w-64 flex-col items-start gap-1">
              {steps.map((step, index) =>
                far(index) ? (
                  <span key={step.id} className="flex flex-col items-start gap-0.5">
                    <span className="font-medium">{step.title}</span>
                    {step.description ? <span>{step.description}</span> : null}
                    <AgentLines agent={task.agents.find((agent) => agent.step === step.id)} />
                  </span>
                ) : null,
              )}
            </TooltipContent>
          </Tooltip>
        </li>
      )}
    </ol>
  )
}

function Arrow() {
  return <ChevronRightIcon aria-hidden className="size-3 shrink-0 text-muted-foreground/60" />
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
