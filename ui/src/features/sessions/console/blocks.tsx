import {
  BrainIcon,
  CheckIcon,
  CircleIcon,
  FileTextIcon,
  GlobeIcon,
  Loader2Icon,
  type LucideIcon,
  MoveRightIcon,
  PencilIcon,
  SearchIcon,
  TerminalIcon,
  Trash2Icon,
  WrenchIcon,
  XIcon,
} from "lucide-react"
import { isValidElement, type KeyboardEvent, memo, type ReactNode } from "react"

import { CodeBlock } from "@/components/code-block"
import { Markdown } from "@/components/markdown"
import { cn, formatClock, formatStamp } from "@/lib/format"

import { Foldable } from "./foldable"
import type { Block, PermissionOption } from "./transcript"

/** The characters of a thought or a daemon prompt its folded summary quotes. */
const SUMMARY_CHARS = 90

/** The first line of a text, cut where a summary line has room for it. */
function firstLine(text: string): string {
  const line = text.split("\n").find((one) => one.trim() !== "") ?? ""
  return line.length > SUMMARY_CHARS ? `${line.slice(0, SUMMARY_CHARS).trimEnd()}…` : line.trim()
}

function lineCount(text: string): string {
  const count = text.replace(/\n$/, "").split("\n").length
  return `${count} ${count === 1 ? "line" : "lines"}`
}

/** How long a call took, in the unit that fits it, as the CLI's pane says it. */
function duration(from: string | undefined, to: string | undefined): string | undefined {
  if (from === undefined || to === undefined) return undefined
  const ms = Date.parse(to) - Date.parse(from)
  if (Number.isNaN(ms) || ms < 0) return undefined
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${Math.floor(ms / 60_000)}m ${String(Math.floor((ms % 60_000) / 1000)).padStart(2, "0")}s`
}

/** A summary line: what the block is at the left, and its muted facts and time at the right end. */
export function SummaryLine({ start, end }: { start: ReactNode; end: (string | undefined)[] }) {
  return (
    <span className="flex min-w-0 items-start gap-3">
      <span className="min-w-0 flex-1 break-words">{start}</span>
      <span data-summary-end className="flex shrink-0 gap-2 tabular-nums">
        {end.map((fact, index) =>
          // biome-ignore lint/suspicious/noArrayIndexKey: each place is one fact, never reordered
          fact ? <span key={index}>{fact}</span> : null,
        )}
      </span>
    </span>
  )
}

/** What a person typed is a message: set apart and full size, with the time it was sent. */
export function PromptBox({
  text,
  queued,
  time,
}: {
  text: string
  queued?: boolean
  time?: string
}) {
  return (
    <div className="rounded-md border-l-2 border-primary bg-primary/10 px-3 py-2">
      {queued ? (
        <span className="mb-1 inline-block rounded bg-muted px-1 font-mono text-[11px] text-muted-foreground">
          queued
        </span>
      ) : null}
      <div className="flex items-start gap-3">
        <Markdown className="min-w-0 flex-1">{text}</Markdown>
        {time ? (
          <span
            data-block-time
            className="shrink-0 pt-0.5 font-mono text-[11px] text-muted-foreground tabular-nums"
          >
            {time}
          </span>
        ) : null}
      </div>
    </div>
  )
}

/** Whether two pickers draw the same: the same node, or one element type with the same props. */
function samePicker(before: ReactNode, after: ReactNode): boolean {
  if (Object.is(before, after)) return true
  if (!isValidElement(before) || !isValidElement(after)) return false
  if (before.type !== after.type || before.key !== after.key) return false
  const was = before.props as Record<string, unknown>
  const now = after.props as Record<string, unknown>
  const keys = Object.keys(was)
  return (
    keys.length === Object.keys(now).length && keys.every((key) => Object.is(was[key], now[key]))
  )
}

type BlockViewProps = {
  block: Block
  picker: ReactNode
  folded: boolean
  onToggle: (id: string, folded: boolean) => void
}

/**
 * One block of the transcript. Memoized: the transcript keeps every block it
 * did not change, and a picker is compared by its props, so a streamed chunk
 * re-renders the one block it grows.
 */
export const BlockView = memo(function BlockView({
  block,
  picker,
  folded,
  onToggle,
}: BlockViewProps) {
  return (
    <div data-block={block.type} data-block-id={block.id} title={formatStamp(block.createdAt)}>
      {block.subagent ? (
        <div className="pb-0.5 font-mono text-[11px] text-muted-foreground">
          ↳ {block.subagent.title}
        </div>
      ) : null}
      <BlockBody
        block={block}
        picker={picker}
        folded={folded}
        onToggle={() => onToggle(block.id, folded)}
      />
    </div>
  )
}, sameBlockView)

function sameBlockView(before: BlockViewProps, after: BlockViewProps): boolean {
  return (
    before.block === after.block &&
    before.folded === after.folded &&
    before.onToggle === after.onToggle &&
    samePicker(before.picker, after.picker)
  )
}

function BlockBody({
  block,
  picker,
  folded,
  onToggle,
}: {
  block: Block
  picker: ReactNode
  folded: boolean
  onToggle: () => void
}) {
  const time = formatClock(block.createdAt)
  switch (block.type) {
    case "prompt":
      return block.daemon ? (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          className="rounded-md border-l-2 bg-muted/40"
          summary={
            <SummaryLine
              start={
                <>
                  daemon prompt · {firstLine(block.text)} · {lineCount(block.text)}
                </>
              }
              end={[time]}
            />
          }
        >
          <Markdown>{block.text}</Markdown>
        </Foldable>
      ) : (
        <PromptBox text={block.text} time={time} />
      )
    case "message":
      return <Markdown>{block.text}</Markdown>
    case "thought":
      return (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          summary={<SummaryLine start={<>thought · {firstLine(block.text)}</>} end={[time]} />}
        >
          <Markdown className="text-muted-foreground italic">{block.text}</Markdown>
        </Foldable>
      )
    case "plan":
      return (
        <ul className="space-y-0.5 font-mono text-xs">
          {block.entries.map((entry, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: a plan's entries have no id, and are replaced whole
            <li key={index} className={cn(entry.status === "completed" && "text-muted-foreground")}>
              <span aria-hidden className="pr-1.5">
                {PLAN_MARKS[entry.status] ?? "○"}
              </span>
              {entry.content} <span className="text-muted-foreground">· {entry.status}</span>
            </li>
          ))}
        </ul>
      )
    case "tool": {
      const { tool } = block
      // A read's subject names its file, so its input says nothing more.
      const input = tool.kind === "read" ? undefined : tool.inputText
      const summary = (
        <SummaryLine
          start={
            <>
              <span className="mr-1.5 inline-flex items-center gap-1.5 align-[-2px]">
                <ToolStatus status={tool.status} />
                <ToolKind kind={tool.kind} />
              </span>
              <span className="font-medium text-foreground">{tool.subject ?? tool.title}</span>
            </>
          }
          end={[
            tool.output ? lineCount(tool.output) : undefined,
            duration(tool.startedAt, tool.endedAt),
            formatClock(tool.startedAt ?? block.createdAt),
          ]}
        />
      )
      if (!input && !tool.output && !tool.diff) {
        return (
          <div className="flex items-start gap-1.5 rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
            <span aria-hidden className="size-3.5 shrink-0" />
            <span className="min-w-0 flex-1">{summary}</span>
          </div>
        )
      }
      return (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          summary={summary}
          className="rounded-md border"
        >
          <div className="space-y-2.5">
            {input ? (
              <CodeBlock code={input} language={tool.inputLanguage} caption="input" />
            ) : null}
            {tool.output ? (
              <CodeBlock
                code={tool.output}
                language={tool.outputLanguage}
                firstLine={tool.firstLine}
                caption="output"
              />
            ) : null}
            {tool.diff ? <CodeBlock code={tool.diff} diff caption="diff" /> : null}
          </div>
        </Foldable>
      )
    }
    case "permission":
      return (
        <div className="rounded-md border border-status-warn/40 bg-status-warn-soft/30 px-3 py-2 text-xs">
          <div className="font-medium">Permission · {block.title}</div>
          {block.command ? (
            <CodeBlock code={block.command} language="shell" caption="command" className="mt-1.5" />
          ) : null}
          {block.diff ? (
            <CodeBlock code={block.diff} diff caption="diff" className="mt-1.5" />
          ) : null}
          {block.answer ? (
            <div className="pt-1 font-mono text-muted-foreground">
              <span aria-hidden>↳ </span>
              {block.answer.decidedBy
                ? `${block.answer.name} · decided by ${block.answer.decidedBy}`
                : block.answer.name}
            </div>
          ) : (
            picker
          )}
        </div>
      )
    case "question":
      return picker
    case "note":
      return <div className="font-mono text-xs text-muted-foreground">· {block.text}</div>
    case "child":
      return <div className="font-mono text-xs text-muted-foreground">{block.text}</div>
    case "error":
      return (
        <div className="rounded-md border border-status-danger/40 bg-status-danger-soft/40 px-3 py-2 text-xs text-status-danger-fg">
          <p className="whitespace-pre-wrap break-words">{block.text}</p>
          {block.diagnosis ? <p className="pt-1 text-muted-foreground">{block.diagnosis}</p> : null}
        </div>
      )
    case "raw":
      return (
        <div className="font-mono text-xs text-muted-foreground">
          {block.kind} · {block.summary}
        </div>
      )
  }
}

const PLAN_MARKS: Record<string, string> = { completed: "✓", in_progress: "◐", pending: "○" }

/** The mark of a call's status, with its name for a screen reader. */
function ToolStatus({ status }: { status?: string }) {
  const [Icon, tone] =
    status === "failed"
      ? [XIcon, "text-status-danger-fg"]
      : status === "completed"
        ? [CheckIcon, "text-status-done-fg"]
        : status === "in_progress"
          ? [Loader2Icon, "animate-spin text-status-active-fg"]
          : [CircleIcon, "text-muted-foreground"]
  return (
    <>
      <Icon aria-hidden className={cn("size-3.5 shrink-0", tone)} />
      <span className="sr-only">{status ?? "pending"}</span>
    </>
  )
}

/** One icon per ACP kind, so the eye tells a command from a read from an edit before a word. */
const KIND_ICONS: Record<string, LucideIcon> = {
  read: FileTextIcon,
  edit: PencilIcon,
  delete: Trash2Icon,
  move: MoveRightIcon,
  search: SearchIcon,
  execute: TerminalIcon,
  think: BrainIcon,
  fetch: GlobeIcon,
}

function ToolKind({ kind }: { kind?: string }) {
  const Icon =
    (kind !== undefined && Object.hasOwn(KIND_ICONS, kind) && KIND_ICONS[kind]) || WrenchIcon
  return (
    <span data-tool-kind={kind ?? "other"} title={kind ?? "other"} className="inline-flex">
      <Icon aria-hidden className="size-3.5 shrink-0" />
      <span className="sr-only">{kind ?? "other"}</span>
    </span>
  )
}

export function PermissionPicker({
  options,
  selected,
  answering,
  onSelect,
  onAnswer,
  onKeyDown,
}: {
  options: PermissionOption[]
  selected: number
  answering: string | null
  onSelect: (index: number) => void
  onAnswer: (option: PermissionOption) => void
  onKeyDown: (event: KeyboardEvent) => boolean
}) {
  return (
    <div
      role="listbox"
      aria-label="Answer the permission"
      tabIndex={0}
      onKeyDown={onKeyDown}
      className="mt-2 space-y-0.5 rounded-md outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
    >
      {options.map((option, index) => (
        <div
          key={option.id}
          role="option"
          aria-selected={index === selected}
          aria-disabled={answering !== null}
          tabIndex={-1}
          onMouseEnter={() => onSelect(index)}
          onClick={() => answering === null && onAnswer(option)}
          onKeyDown={() => undefined}
          className={cn(
            "flex cursor-pointer items-center gap-2 rounded px-2 py-1 font-mono",
            index === selected ? "bg-primary text-primary-foreground" : "hover:bg-muted",
          )}
        >
          <span className="w-4 text-right opacity-70">{index + 1}.</span>
          {option.name}
          {answering === option.id ? (
            <Loader2Icon className="size-3 animate-spin" aria-hidden />
          ) : null}
        </div>
      ))}
    </div>
  )
}
