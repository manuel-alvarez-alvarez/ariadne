/**
 * One step of a turn as a row: the icon of its status, the icon of its kind,
 * its title, its subject and its duration. The words of the status and the
 * kind are the icons' tooltips and stay in the row's accessible name. An
 * edit adds the lines it added and removed, and a subagent's row the count
 * of its steps. With `onToggle` the row is a disclosure that opens the
 * step's detail under it.
 *
 * A group of read and search calls (`step-groups.ts`) is a row of its own:
 * its state, its kind and the subject of each call, cut to the row.
 */

import type { ReactNode } from "react"

import type { StepDto } from "@/api"
import { NamedIcon } from "@/components/named-icon"
import { cn, formatClock } from "@/lib/format"

import { elapsed } from "../console/elapsed"
import { PermissionMark, type PermissionStep } from "./notices"
import { groupState, type ToolStep } from "./step-groups"
import { GROUP_ICON, kindIcon, StateIcon } from "./step-icons"
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
  stat?: NonNullable<ToolStep["diff_stat"]>
} {
  switch (step.type) {
    case "tool_call":
      return {
        kind: step.kind ?? "tool",
        status: step.status,
        title: firstLine(step.title),
        subject: subjectBeside(step.title, step.subject),
        duration: elapsed(step.started_at, step.ended_at ?? now),
        stat: step.kind === "edit" ? (step.diff_stat ?? undefined) : undefined,
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
        title: firstLine(step.title),
        subject: step.task ? subjectBeside(step.title, step.task) : undefined,
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

/**
 * The cells of a row with a space between each two. The row is a flex box,
 * which draws no space of text, but the space keeps the words of the
 * accessible name apart.
 */
function spaced(cells: readonly ReactNode[]): ReactNode[] {
  return cells.filter(Boolean).flatMap((cell, at) => (at === 0 ? [cell] : [" ", cell]))
}

/** The first line of a subject, unless the first line of the title already holds it. */
function subjectBeside(title: string, subject?: string | null): string | undefined {
  const line = subject ? firstLine(subject) : ""
  return line && !firstLine(title).includes(line) ? line : undefined
}

function firstLine(text: string): string {
  return (
    text
      .split("\n")
      .find((line) => line.trim().length > 0)
      ?.trim() ?? ""
  )
}

const ROW = "flex min-w-0 items-center gap-2 rounded-md px-2 py-0.5 font-mono text-xs"
const DISCLOSURE =
  "w-full min-w-0 flex-1 cursor-pointer text-left outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50"

/** One step as a row: the status, the kind, the title, the subject and the duration. */
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
  const cells = spaced([
    <StateIcon key="state" state={facts.status} />,
    <NamedIcon
      key="kind"
      icon={kindIcon(facts.kind, step.type === "tool_call")}
      label={facts.kind}
      className="text-muted-foreground"
    />,
    <span key="title" className="min-w-0 truncate text-foreground">
      {facts.title}
    </span>,
    facts.subject ? (
      <span key="subject" className="min-w-0 truncate text-muted-foreground">
        {facts.subject}
      </span>
    ) : null,
    facts.stat ? (
      <span key="stat" className="shrink-0 tabular-nums">
        <span className="text-diff-add-fg">+{facts.stat.added}</span>{" "}
        <span className="text-diff-remove-fg">-{facts.stat.removed}</span>
      </span>
    ) : null,
    permission ? <PermissionMark key="permission" step={permission} /> : null,
    facts.count || facts.duration ? (
      <span key="time" className="ml-auto flex shrink-0 gap-2 tabular-nums text-muted-foreground">
        {spaced([
          facts.count ? <span key="count">{facts.count}</span> : null,
          facts.duration ? <span key="duration">{facts.duration}</span> : null,
        ])}
      </span>
    ) : null,
  ])
  const shared = {
    "data-step-id": step.id,
    "data-step-kind": step.type,
    title: formatClock(step.created_at),
  }
  if (!onToggle)
    return (
      <div {...shared} className={ROW}>
        {cells}
      </div>
    )
  return (
    <button
      type="button"
      {...shared}
      aria-expanded={open === true}
      onClick={onToggle}
      className={cn(ROW, DISCLOSURE, open && "bg-muted/40")}
    >
      {cells}
    </button>
  )
}

/**
 * A group of read and search calls as one row: its state, the icon of its
 * kind (or of a group, where it holds both) and the subject of each call,
 * cut to the row. It is a disclosure that opens to the rows of its calls.
 */
export function GroupRow({
  steps,
  open,
  onToggle,
}: {
  steps: readonly ToolStep[]
  open: boolean
  onToggle: () => void
}) {
  const kinds = new Set(steps.map((step) => step.kind))
  const [kind] = kinds
  const subjects = steps.map((step) => firstLine(step.subject || step.title)).join(", ")
  return (
    <button
      type="button"
      data-step-group={steps[0]?.id}
      aria-expanded={open}
      onClick={onToggle}
      className={cn(ROW, DISCLOSURE, open && "bg-muted/40")}
    >
      {spaced([
        <StateIcon key="state" state={groupState(steps)} />,
        <NamedIcon
          key="kind"
          icon={kinds.size === 1 && kind ? kindIcon(kind, true) : GROUP_ICON}
          label={kinds.size === 1 && kind ? kind : "read and search"}
          className="text-muted-foreground"
        />,
        <span key="subjects" data-group-subjects className="min-w-0 truncate text-foreground">
          {subjects}
        </span>,
      ])}
    </button>
  )
}
