/**
 * Where a stepped task is in its goal's workflow, and who is running each
 * column — the one section that replaces the old Agents fact and step strip,
 * which repeated the same columns as two separate displays.
 *
 * Closed, a row is its state (done, current or pending) and its title: the
 * progress at a glance. Opened, it adds what the strip only put in a hover
 * hint and the fact only put in a separate list: the column's description,
 * its agent's skills, model and effort, and a link to its live session.
 *
 * A task still pending is in no column yet, so nothing is current; a
 * finished task has been through every one.
 */

import { CheckIcon } from "lucide-react"
import type { ReactNode } from "react"

import type { TaskDto, WorkflowStepDto } from "@/api"
import { StatusBadge } from "@/components/status-badge"
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion"
import { AgentSummary } from "@/features/models/agent-summary"
import { SessionLink } from "./task-sessions"

type StepState = "done" | "current" | "ahead"

const STATE_META: Record<
  StepState,
  { label: string; tone: string; dot?: string; icon?: ReactNode; pulse?: boolean }
> = {
  done: {
    label: "Done",
    tone: "bg-status-done-soft text-status-done-fg",
    icon: <CheckIcon className="size-3" aria-hidden />,
  },
  current: {
    label: "Current",
    tone: "bg-status-active-soft text-status-active-fg",
    dot: "bg-status-active",
    pulse: true,
  },
  ahead: {
    label: "Pending",
    tone: "bg-muted text-muted-foreground",
    dot: "bg-muted-foreground/60",
  },
}

export function WorkflowSteps({ task, steps }: { task: TaskDto; steps: WorkflowStepDto[] }) {
  const current = steps.findIndex((step) => step.id === task.step)
  return (
    <Accordion className="rounded-lg border" multiple>
      {steps.map((step, index) => {
        const state = stepState(task, index, current)
        const meta = STATE_META[state]
        const agent = task.agents.find((candidate) => candidate.step === step.id)
        return (
          <AccordionItem key={step.id} value={step.id} className="px-3">
            <AccordionTrigger
              aria-current={state === "current" ? "step" : undefined}
              className="gap-3"
            >
              <span className="min-w-0 flex-1 text-left break-words">{step.title}</span>
              <StatusBadge
                size="sm"
                label={meta.label}
                tone={meta.tone}
                dot={meta.dot}
                icon={meta.icon}
                pulse={meta.pulse}
              />
            </AccordionTrigger>
            <AccordionContent>
              <div className="flex flex-col gap-2 text-xs">
                {step.description ? (
                  <p className="text-muted-foreground">{step.description}</p>
                ) : null}
                {agent ? (
                  <>
                    <AgentSummary skills={agent.skills} model={agent.model} effort={agent.effort} />
                    {agent.session_id ? <SessionLink sessionId={agent.session_id} /> : null}
                  </>
                ) : (
                  <span className="text-muted-foreground italic">none staffed</span>
                )}
              </div>
            </AccordionContent>
          </AccordionItem>
        )
      })}
    </Accordion>
  )
}

function stepState(task: TaskDto, index: number, current: number): StepState {
  if (task.status === "finished") return "done"
  if (current < 0) return "ahead"
  if (index < current) return "done"
  return index === current ? "current" : "ahead"
}
