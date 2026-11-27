import { PermissionMark, type PermissionStep } from "./notices"
/**
 * One step of a turn as a row: its kind, its status, its title, its subject
 * and its duration. A subagent's row adds the count of its steps. With
 * `onToggle` the row is a disclosure that opens the step's detail under it.
 */

import type { StepDto } from "@/api"
import { cn, formatClock } from "@/lib/format"

import { elapsed } from "../console/elapsed"
import { contentText } from "./store"

/** What a row of a step says: its kind, its status, its title, its subject and its duration. */
function stepFacts(
  step: StepDto,
  now: string,
): {
  kind: string
  status: string
  title: string
  subject?: string
  duration?: string
  count?: string
} {
  switch (step.type) {
    case "tool_call":
      return {
        kind: step.kind ?? "tool",
        status: step.status,
        title: step.title,
        subject: step.subject && step.subject !== step.title ? firstLine(step.subject) : undefined,
        duration: elapsed(step.started_at, step.ended_at ?? now),
      }
    case "message":
    case "thought":
      return {
        kind: step.type,
        status: step.streaming ? "running" : "completed",
        title: firstLine(contentText(step.content)),
      }
    case "permission":
      return {
        kind: "permission",
        status: step.answer ? `${step.state} · ${step.answer.label}` : step.state,
        title: step.tool.title,
        subject: step.tool.subject ? firstLine(step.tool.subject) : undefined,
      }
    case "question":
      return {
        kind: "question",
        status: step.state,
        title: firstLine(step.question),
        subject: step.hint || undefined,
      }
    case "note":
      return { kind: "note", status: step.severity, title: firstLine(step.text) }
    case "error":
      return {
        kind: "error",
        status: "failed",
        title: firstLine(step.text),
        subject: step.diagnosis ? firstLine(step.diagnosis) : undefined,
      }
    case "subagent":
      return {
        kind: "subagent",
        status: step.state ?? "running",
        title: step.title,
        subject: step.task ? firstLine(step.task) : undefined,
        duration: elapsed(step.started_at, step.ended_at ?? now),
        count: `${step.activity.steps} ${step.activity.steps === 1 ? "step" : "steps"}`,
      }
    case "compaction":
      return {
        kind: "compaction",
        status: step.status,
        title: "compaction",
        subject: step.error ?? undefined,
      }
  }
}

function firstLine(text: string): string {
  return (
    text
      .split("\n")
      .find((line) => line.trim().length > 0)
      ?.trim() ?? ""
  )
}

const STATUS_TONES: Record<string, string> = {
  running: "text-status-active-fg",
  pending: "text-status-warn-fg",
  open: "text-status-warn-fg",
  warning: "text-status-warn-fg",
  failed: "text-status-danger-fg",
  interrupted: "text-status-danger-fg",
  completed: "text-status-done-fg",
}

/** One step as a row: the kind, the status, the title, the subject and the duration. */
export function StepRow({
  step,
  now,
  open,
  onToggle,
  permission,
}: {
  step: StepDto
  now: string
  permission?: PermissionStep
  /** Whether the step's detail is open under the row. */
  open?: boolean
  /** Open or close the detail: the row is then a disclosure. */
  onToggle?: () => void
}) {
  const facts = stepFacts(step, now)
  const tone = STATUS_TONES[facts.status.split(" ")[0] ?? ""] ?? "text-muted-foreground"
  const cells = (
    <>
      <span className="shrink-0 text-muted-foreground">{facts.kind}</span>
      <span className={cn("shrink-0", tone)}>{facts.status}</span>
      <span className="min-w-0 truncate text-foreground">{facts.title}</span>
      {facts.subject ? (
        <span className="min-w-0 truncate text-muted-foreground">{facts.subject}</span>
      ) : null}
      {permission ? <PermissionMark step={permission} /> : null}
      {facts.count || facts.duration ? (
        <span className="ml-auto flex shrink-0 gap-2 tabular-nums text-muted-foreground">
          {facts.count ? <span>{facts.count}</span> : null}
          {facts.duration ? <span>{facts.duration}</span> : null}
        </span>
      ) : null}
    </>
  )
  const shared = {
    "data-step-id": step.id,
    "data-step-kind": step.type,
    title: formatClock(step.created_at),
  }
  const layout = "flex min-w-0 items-baseline gap-2 rounded-md px-2 py-0.5 font-mono text-xs"
  if (!onToggle)
    return (
      <div {...shared} className={layout}>
        {cells}
      </div>
    )
  return (
    <button
      type="button"
      {...shared}
      aria-expanded={open === true}
      onClick={onToggle}
      className={cn(
        layout,
        "w-full min-w-0 flex-1 cursor-pointer text-left outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50",
        open && "bg-muted/40",
      )}
    >
      {cells}
    </button>
  )
}
