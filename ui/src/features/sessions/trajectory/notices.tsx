/**
 * The notices of a turn, and the mark of an answered permission.
 *
 * A warning note and a compaction always show in their turn, as a line of
 * their own, whatever the fold: the console takes them from the `notices`
 * of the turn, and where `notices_more` counts others, one row opens the
 * steps of the turn. It reads no page of steps to find them. An info note is a row of the open
 * activity fold alone. An error is a step of the turn's `attention`, which
 * `turn-view.tsx` draws.
 *
 * An answered permission is no row of its own: it is a mark on the row of its
 * call, which says the option chosen and who chose it.
 */

import { CircleAlertIcon, Loader2Icon, PackageIcon } from "lucide-react"

import type { StepDto } from "@/api"
import { cn } from "@/lib/format"

type NoteStep = Extract<StepDto, { type: "note" }>
type CompactionStep = Extract<StepDto, { type: "compaction" }>
export type PermissionStep = Extract<StepDto, { type: "permission" }>
type ToolStep = Extract<StepDto, { type: "tool_call" }>

/** A step that shows as a line of its turn whatever the fold: a warning or a compaction. */
export function isNotice(step: StepDto): step is NoteStep | CompactionStep {
  return (step.type === "note" && step.severity === "warning") || step.type === "compaction"
}

/** A step that shows only as a row of the open fold: an info note. */
export function isInfoNote(step: StepDto): boolean {
  return step.type === "note" && step.severity === "info"
}

export function NoticeLine({ step }: { step: NoteStep | CompactionStep }) {
  if (step.type === "note")
    return (
      <div
        data-notice="warning"
        data-step-id={step.id}
        className="ml-4 flex min-w-0 items-baseline gap-2 rounded-md bg-status-warn-soft/40 px-2 py-0.5 font-mono text-xs text-status-warn-fg"
      >
        <CircleAlertIcon aria-hidden className="size-3 shrink-0 self-center" />
        <span className="shrink-0">warning</span>
        <span className="min-w-0 break-words text-foreground">{step.text}</span>
      </div>
    )
  const failed = step.status === "failed"
  const Icon = step.status === "running" ? Loader2Icon : PackageIcon
  return (
    <div
      data-notice="compaction"
      data-step-id={step.id}
      className={cn(
        "ml-4 flex min-w-0 items-baseline gap-2 rounded-md px-2 py-0.5 font-mono text-xs",
        failed ? "text-status-danger-fg" : "text-muted-foreground",
      )}
    >
      <Icon
        aria-hidden
        className={cn("size-3 shrink-0 self-center", step.status === "running" && "animate-spin")}
      />
      <span className="min-w-0 break-words">
        {step.status === "running"
          ? "Compacting the context…"
          : failed
            ? `Could not compact the context${step.error ? `: ${step.error}` : ""}`
            : "Compacted the context"}
      </span>
    </div>
  )
}

/**
 * The answered permissions among `steps`, by the call each asked about: by
 * the call's `permission` id, and by its tool call id.
 */
export function answeredPermissions(steps: readonly StepDto[]): Map<string, PermissionStep> {
  const byCall = new Map<string, PermissionStep>()
  for (const step of steps) {
    if (step.type !== "permission" || step.state !== "answered" || !step.answer) continue
    byCall.set(`permission:${step.id}`, step)
    byCall.set(`call:${step.tool_call_id}`, step)
  }
  return byCall
}

/** The answered permission of a call, where `answered` holds one. */
export function permissionOf(
  call: ToolStep,
  answered: ReadonlyMap<string, PermissionStep>,
): PermissionStep | undefined {
  return (
    (call.permission ? answered.get(`permission:${call.permission}`) : undefined) ??
    answered.get(`call:${call.tool_call_id}`)
  )
}

const DECIDERS: Record<string, string> = {
  console: "you",
  ai: "AI",
  learned: "learned rule",
  auto: "auto",
}

/** Who chose the answer, as a reader says it. */
function decider(step: PermissionStep): string | undefined {
  const by = step.answer?.decided_by
  if (!by) return undefined
  if (by === "learned" && step.learned) return `learned rule ${step.learned}`
  return DECIDERS[by] ?? by
}

/**
 * The answer of a permission and who decided it, as the detail of a call
 * shows it, whoever decided: `Allow once · auto`.
 */
export function permissionAnswer(step: PermissionStep): string | undefined {
  if (!step.answer) return undefined
  const who = decider(step)
  return `${step.answer.label}${who ? ` · ${who}` : ""}`
}

/**
 * The mark of an answered permission on its call's row: `✓ Allow once · you`.
 * An allow that `auto` or a learned rule gave has no mark; an allow by the AI
 * or a person is muted, and a rejection is the danger colour.
 */
export function PermissionMark({ step }: { step: PermissionStep }) {
  const answer = step.answer
  if (!answer) return null
  const kind = step.options.find((option) => option.id === answer.option_id)?.kind ?? ""
  const rejected = kind.startsWith("reject")
  // `auto` and a learned rule allow as a matter of course: only the rest is news.
  if (!rejected && (answer.decided_by === "auto" || answer.decided_by === "learned")) return null
  const who = decider(step)
  return (
    <span
      data-permission-mark
      title={step.ai ?? undefined}
      className={cn("shrink-0", rejected ? "text-status-danger-fg" : "text-muted-foreground")}
    >
      {rejected ? "✗" : "✓"} {answer.label}
      {who ? ` · ${who}` : ""}
    </span>
  )
}
