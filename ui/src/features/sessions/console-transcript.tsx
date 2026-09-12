/**
 * A session's console, drawn the way a terminal draws a session: one column,
 * in the pane's monospace face, each item a row of its own kind.
 *
 * The rows are the items `console-items.ts` folds the stream into. A prompt
 * is a `>` line. Agent text is markdown. A thought is dimmed and folded to
 * two lines until it is opened. A tool call is one row — a status glyph, the
 * name, the input — that opens to what the tool showed, a diff included. A
 * plan is a checklist. A permission question lists its options inline, each
 * with the number key that picks it. Anything the fold did not recognize
 * keeps the one-line-summary-plus-payload row of the agent activity feed.
 */

import {
  CheckIcon,
  ChevronRightIcon,
  CircleIcon,
  Loader2Icon,
  OctagonXIcon,
  SquareIcon,
  XIcon,
} from "lucide-react"
import { type ReactNode, useState } from "react"

import type { AgentEventDto } from "@/api"
import { Markdown } from "@/components/markdown"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { When } from "@/components/when"
import { cn } from "@/lib/format"

import { ConsoleDiff } from "./console-diff"
import type { ConsoleItem, PlanEntry, ToolCall, ToolState } from "./console-items"

/** How a still-open permission question is answered from a row. */
export interface ConsoleContext {
  /** The permission item a submitted answer is still in flight for. */
  answeringId: string | null
  /** An answer this console gave, shown until the daemon's own reply confirms it. */
  localAnswers: ReadonlyMap<string, string>
  onAnswerPermission: (itemId: string, optionId: string, label: string) => void
}

type PermissionItem = Extract<ConsoleItem, { kind: "permission" }>

export function ConsoleTranscript({
  items,
  context,
}: {
  items: ConsoleItem[]
  context: ConsoleContext
}) {
  return (
    <ol className="flex flex-col gap-1.5">
      {items.map((item) => (
        <li key={item.id}>
          <ConsoleRow item={item} context={context} />
        </li>
      ))}
    </ol>
  )
}

function ConsoleRow({ item, context }: { item: ConsoleItem; context: ConsoleContext }) {
  switch (item.kind) {
    case "prompt":
      return item.source === "daemon" ? (
        <PromptLine dimmed>
          <FoldedText label="Prompt from Ariadne">{item.text}</FoldedText>
        </PromptLine>
      ) : (
        <PromptLine>{item.text}</PromptLine>
      )
    case "message":
      return (
        <Markdown className="font-mono text-xs [&_h1]:font-mono [&_h2]:font-mono [&_h3]:font-mono">
          {item.text}
        </Markdown>
      )
    case "thought":
      return (
        <div className="text-muted-foreground">
          <FoldedText label="Thought">{item.text}</FoldedText>
        </div>
      )
    case "tool":
      return <ToolRow call={item.call} state={item.state} />
    case "plan":
      return <PlanList entries={item.entries} />
    case "permission":
      return <PermissionRow item={item} context={context} />
    case "note":
      return (
        <p
          className={cn(
            "flex items-center gap-1.5 py-0.5",
            item.tone === "stopped" ? "text-status-warn-fg" : "text-muted-foreground",
          )}
        >
          <SquareIcon className="size-2 shrink-0" aria-hidden />
          {item.text}
        </p>
      )
    case "error":
      return (
        <p className="flex items-center gap-1.5 py-0.5 text-status-danger-fg">
          <OctagonXIcon className="size-3 shrink-0" aria-hidden />
          The agent reported an error: {item.text}
        </p>
      )
    case "raw":
      return <RawEventRow event={item.event} />
  }
}

/**
 * A prompt as a terminal echoes one: the `>` in the margin, the text after
 * it. Pending — sent, not yet confirmed by its `user_prompt_submit` — while
 * the console waits for the daemon.
 */
export function PromptLine({
  children,
  dimmed,
  pending,
}: {
  children: ReactNode
  dimmed?: boolean
  pending?: boolean
}) {
  return (
    <div
      aria-busy={pending ? "true" : undefined}
      className={cn(
        "flex gap-2 whitespace-pre-wrap",
        (dimmed || pending) && "text-muted-foreground",
      )}
    >
      <span className="shrink-0 select-none text-primary" aria-hidden>
        &gt;
      </span>
      <div className="min-w-0 flex-1 break-words">{children}</div>
      {pending ? (
        <span className="flex shrink-0 items-center">
          <Loader2Icon className="size-3 animate-spin" aria-hidden />
          <span className="sr-only">Sending…</span>
        </span>
      ) : null}
    </div>
  )
}

/** Text folded to two lines, with a toggle that opens the whole of it. */
function FoldedText({ label, children }: { label: string; children: string }) {
  const [open, setOpen] = useState(false)
  return (
    <div>
      <button
        type="button"
        className="flex items-center gap-1 text-left opacity-80 hover:opacity-100"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <ChevronRightIcon
          className={cn("size-3 shrink-0 transition-transform", open && "rotate-90")}
          aria-hidden
        />
        {label}
      </button>
      <div className={cn("whitespace-pre-wrap break-words", !open && "line-clamp-2")}>
        {children}
      </div>
    </div>
  )
}

const TOOL_STATE_LABEL: Record<ToolState, string> = {
  running: "Running",
  done: "Done",
  failed: "Failed",
}

function ToolRow({ call, state }: { call: ToolCall; state: ToolState }) {
  const [open, setOpen] = useState(false)
  return (
    <div>
      <button
        type="button"
        className="flex w-full items-center gap-2 text-left"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <ChevronRightIcon
          className={cn(
            "size-3 shrink-0 text-muted-foreground transition-transform",
            open && "rotate-90",
          )}
          aria-hidden
        />
        <StatusGlyph state={state} />
        <span className="shrink-0 font-medium">{call.title}</span>
        <span className="min-w-0 flex-1 truncate text-muted-foreground">
          {inputSummary(call.rawInput)}
        </span>
      </button>
      {open ? (
        <div className="mt-1 ml-5 flex flex-col gap-2">
          <ToolOutput call={call} />
        </div>
      ) : null}
    </div>
  )
}

function StatusGlyph({ state }: { state: ToolState }) {
  const Icon = state === "running" ? Loader2Icon : state === "done" ? CheckIcon : XIcon
  return (
    <span
      className={cn(
        "flex shrink-0 items-center",
        state === "running" && "text-muted-foreground",
        state === "done" && "text-status-done-fg",
        state === "failed" && "text-status-danger-fg",
      )}
    >
      <Icon className={cn("size-3", state === "running" && "animate-spin")} aria-hidden />
      <span className="sr-only">{TOOL_STATE_LABEL[state]}</span>
    </span>
  )
}

/** What the tool showed: its content entries, or its raw output where it gave none. */
function ToolOutput({ call }: { call: ToolCall }) {
  if (call.content.length > 0) {
    return call.content.map((entry, index) => {
      const key = `${call.toolCallId}-${index}`
      if (entry.type === "diff") {
        return (
          <ConsoleDiff
            key={key}
            path={entry.path}
            oldText={entry.oldText}
            newText={entry.newText}
          />
        )
      }
      return <Output key={key}>{entry.type === "text" ? entry.text : stringify(entry.raw)}</Output>
    })
  }
  if (call.rawOutput !== undefined) return <Output>{stringify(call.rawOutput)}</Output>
  return <p className="text-muted-foreground">No output yet.</p>
}

function Output({ children }: { children: string }) {
  return (
    <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted/40 p-2">
      {children}
    </pre>
  )
}

const PLAN_STATUS_LABEL: Record<PlanEntry["status"], string> = {
  pending: "Pending",
  in_progress: "In progress",
  completed: "Done",
}

function PlanList({ entries }: { entries: PlanEntry[] }) {
  return (
    <ol aria-label="Plan" className="flex flex-col gap-0.5">
      {entries.map((entry, index) => {
        const Icon =
          entry.status === "completed"
            ? CheckIcon
            : entry.status === "in_progress"
              ? Loader2Icon
              : CircleIcon
        return (
          // Entries carry no id and a plan update replaces the whole list, so
          // an index key relabels rows rather than mispairing them.
          // biome-ignore lint/suspicious/noArrayIndexKey: plan entries carry no id
          <li key={index} className="flex items-center gap-2">
            <Icon
              className={cn(
                "size-3 shrink-0",
                entry.status === "completed" && "text-status-done-fg",
                entry.status === "in_progress" && "animate-spin",
                entry.status === "pending" && "text-muted-foreground",
              )}
              aria-hidden
            />
            <span className="sr-only">{PLAN_STATUS_LABEL[entry.status]}</span>
            <span className={cn(entry.status === "completed" && "text-muted-foreground")}>
              {entry.content}
            </span>
          </li>
        )
      })}
    </ol>
  )
}

function PermissionRow({ item, context }: { item: PermissionItem; context: ConsoleContext }) {
  // The daemon's own answer wins where both exist: it is the one that reached
  // the agent.
  const answer = item.answer !== undefined ? item.answer : context.localAnswers.get(item.id)
  const answering = context.answeringId === item.id
  return (
    <div className="flex flex-col gap-1.5">
      <p>
        Permission requested: <span className="font-medium">{item.toolName}</span>
      </p>
      {answer !== undefined ? (
        <p className="text-muted-foreground">
          {answer === null
            ? "Cancelled — nothing to select."
            : `Answered: ${item.options.find((option) => option.optionId === answer)?.name ?? answer}`}
        </p>
      ) : (
        <div className="flex flex-wrap gap-2">
          {item.options.map((option, index) => (
            <Button
              key={option.optionId}
              size="xs"
              variant="outline"
              className="font-mono"
              pending={answering}
              disabled={answering}
              onClick={() => context.onAnswerPermission(item.id, option.optionId, option.name)}
            >
              {index < 9 ? (
                <kbd className="rounded bg-muted px-1 text-muted-foreground" aria-hidden>
                  {index + 1}
                </kbd>
              ) : null}
              {option.name}
            </Button>
          ))}
        </div>
      )}
    </div>
  )
}

/** The fallback every event kind gets until it earns a shape of its own. */
function RawEventRow({ event }: { event: AgentEventDto }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="py-0.5">
      <button
        type="button"
        className="flex w-full items-baseline gap-2 text-left"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <ChevronRightIcon
          className={cn(
            "size-3 shrink-0 translate-y-0.5 text-muted-foreground transition-transform",
            open && "rotate-90",
          )}
          aria-hidden
        />
        <Badge variant="secondary" className="shrink-0 font-mono">
          {event.kind}
        </Badge>
        <span className="min-w-0 flex-1 truncate text-muted-foreground">{event.summary}</span>
        <When
          at={event.created_at}
          format="age"
          label="reported"
          className="shrink-0 text-muted-foreground tabular-nums"
        />
      </button>
      {open ? (
        <section
          aria-label={`${event.kind} payload`}
          // biome-ignore lint/a11y/noNoninteractiveTabindex: a scroll container has to take focus to be scrollable by keyboard
          tabIndex={0}
          className="mt-1 max-h-64 overflow-auto rounded-md bg-muted/40 p-2 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          <pre>{stringify(event.payload)}</pre>
        </section>
      ) : null}
    </div>
  )
}

/** A tool's input on one line: a string as it is, anything else as compact JSON. */
function inputSummary(input: unknown): string {
  if (input === undefined || input === null) return ""
  if (typeof input === "string") return input
  if (typeof input === "object") {
    const values = Object.values(input as Record<string, unknown>)
    // The common case — `{command: "ls"}`, `{file_path: "…"}` — reads better
    // as the one value than as its JSON.
    if (values.length === 1 && typeof values[0] === "string") return values[0]
  }
  try {
    return JSON.stringify(input) ?? String(input)
  } catch {
    return String(input)
  }
}

function stringify(payload: unknown): string {
  try {
    return JSON.stringify(payload, null, 2) ?? String(payload)
  } catch {
    return String(payload)
  }
}
