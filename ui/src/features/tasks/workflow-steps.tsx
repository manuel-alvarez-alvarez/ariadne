/**
 * Where a stepped task is in its goal's workflow, and who is running each
 * column, as one stepper in one frame: a track of nodes along the top, one
 * per column in order with the column's title under each, and under the
 * track the picked column's detail, joined to its node by a caret. The whole
 * section is three or four lines tall however many columns the workflow has
 * — the detail of one column replaces the detail of the last rather than
 * stacking under it.
 *
 * A node is its state: a filled check for a column the task has been through,
 * a pulsing dot for the one it is in, a hollow ring for one still ahead — and
 * the track between two nodes is drawn in the completion colour up to the
 * current one. The picked column opens on the current one, and follows the
 * task as it moves until the user picks another; its detail is the column's
 * state in words beside its description, then its agent's skills and model
 * on one line, and the link to its live session.
 *
 * A task still pending is in no column yet, so nothing is current; a
 * finished task has been through every one.
 */

import { Tabs } from "@base-ui/react/tabs"
import { CheckIcon } from "lucide-react"
import { useState } from "react"

import type { TaskDto, WorkflowStepDto } from "@/api"
import { StatusBadge } from "@/components/status-badge"
import { AgentSummary } from "@/features/models/agent-summary"
import { cn } from "@/lib/format"
import { SessionLink } from "./task-sessions"

type StepState = "done" | "current" | "ahead"

const STATE_META: Record<
  StepState,
  {
    label: string
    /** The badge in the picked column's card. */
    tone: string
    /** The node on the track. */
    node: string
    /** The track leaving the node, towards the next one. */
    track: string
  }
> = {
  done: {
    label: "Done",
    tone: "bg-status-done-soft text-status-done-fg",
    node: "border-status-done bg-status-done text-background",
    track: "bg-status-done",
  },
  current: {
    label: "Current",
    tone: "bg-status-active-soft text-status-active-fg",
    node: "border-status-active bg-status-active-soft",
    track: "bg-border",
  },
  ahead: {
    label: "Pending",
    tone: "bg-muted text-muted-foreground",
    node: "border-border bg-background",
    track: "bg-border",
  },
}

export function WorkflowSteps({ task, steps }: { task: TaskDto; steps: WorkflowStepDto[] }) {
  const current = steps.findIndex((step) => step.id === task.step)
  // The user's own pick, or none: with none, the card follows the task along
  // the track as it moves.
  const [picked, setPicked] = useState<string | null>(null)
  const fallback = current >= 0 ? current : task.status === "finished" ? steps.length - 1 : 0
  const selected = steps.some((step) => step.id === picked) ? picked : steps[fallback]?.id

  return (
    <Tabs.Root
      value={selected}
      onValueChange={(value) => setPicked(String(value))}
      className="rounded-lg border bg-card"
    >
      <Tabs.List
        aria-label="Workflow steps"
        activateOnFocus
        className="flex items-start px-2 pt-1.5 pb-1"
      >
        {steps.map((step, index) => {
          const state = stepState(task, index, current)
          const meta = STATE_META[state]
          return (
            <Tabs.Tab
              key={step.id}
              value={step.id}
              aria-current={state === "current" ? "step" : undefined}
              data-step-state={state}
              className="group/step flex min-w-0 flex-1 flex-col items-center gap-1 rounded-md py-1 text-xs text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 data-active:text-foreground"
            >
              <span className="flex w-full items-center">
                <span
                  className={cn(
                    "h-px flex-1",
                    index === 0
                      ? "invisible"
                      : STATE_META[stepState(task, index - 1, current)].track,
                  )}
                  aria-hidden
                />
                <span
                  className={cn(
                    "flex size-5 shrink-0 items-center justify-center rounded-full border-2 transition-shadow group-data-active/step:ring-2 group-data-active/step:ring-ring/40 group-data-active/step:ring-offset-2 group-data-active/step:ring-offset-background",
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
                  className={cn(
                    "h-px flex-1",
                    index === steps.length - 1 ? "invisible" : meta.track,
                  )}
                  aria-hidden
                />
              </span>
              <span className="max-w-full truncate px-1 group-data-active/step:font-medium">
                {step.title}
              </span>
              <span className="sr-only">{meta.label}</span>
            </Tabs.Tab>
          )
        })}
      </Tabs.List>
      {steps.map((step, index) => {
        const meta = STATE_META[stepState(task, index, current)]
        const agent = task.agents.find((candidate) => candidate.step === step.id)
        return (
          <Tabs.Panel
            key={step.id}
            value={step.id}
            className="relative flex flex-col gap-1 border-t px-3 py-2 text-xs outline-none"
          >
            {/* The caret joining the detail to its node: the list's horizontal
                padding is this wrapper's inset, so the same fraction lands on
                the node's centre. */}
            <span className="pointer-events-none absolute inset-x-2 top-0" aria-hidden>
              <span
                className="absolute size-2.5 -translate-x-1/2 -translate-y-1/2 rotate-45 border-t border-l bg-card"
                style={{ left: `${((index + 0.5) / steps.length) * 100}%` }}
              />
            </span>
            <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
              <StatusBadge size="sm" label={meta.label} tone={meta.tone} />
              {step.description ? (
                <span className="min-w-0 flex-1 text-muted-foreground">{step.description}</span>
              ) : null}
              {agent?.session_id ? (
                <span className="ml-auto shrink-0">
                  <SessionLink sessionId={agent.session_id} />
                </span>
              ) : null}
            </div>
            {agent ? (
              <AgentSummary skills={agent.skills} model={agent.model} effort={agent.effort} />
            ) : (
              <span className="text-muted-foreground italic">none staffed</span>
            )}
          </Tabs.Panel>
        )
      })}
    </Tabs.Root>
  )
}

function stepState(task: TaskDto, index: number, current: number): StepState {
  if (task.status === "finished") return "done"
  if (current < 0) return "ahead"
  if (index < current) return "done"
  return index === current ? "current" : "ahead"
}
