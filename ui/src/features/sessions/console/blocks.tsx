import {
  BotIcon,
  BrainIcon,
  CheckIcon,
  CircleIcon,
  CodeIcon,
  FileTextIcon,
  GitBranchIcon,
  GlobeIcon,
  InfoIcon,
  ListIcon,
  Loader2Icon,
  type LucideIcon,
  MessageSquareIcon,
  MoveRightIcon,
  PencilIcon,
  SearchIcon,
  ShieldIcon,
  TerminalIcon,
  Trash2Icon,
  TriangleAlertIcon,
  UserIcon,
  WrenchIcon,
  XIcon,
} from "lucide-react"
import {
  isValidElement,
  type KeyboardEvent,
  memo,
  type ReactNode,
  useEffect,
  useState,
} from "react"

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

/** How long a call took, in the unit that fits it, as the CLI's pane says it. */
function duration(from: string | undefined, to: string | undefined): string | undefined {
  if (from === undefined || to === undefined) return undefined
  const ms = Date.parse(to) - Date.parse(from)
  if (Number.isNaN(ms) || ms < 0) return undefined
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${Math.floor(ms / 60_000)}m ${String(Math.floor((ms % 60_000) / 1000)).padStart(2, "0")}s`
}

/**
 * A summary line: what the block is at the left, and its muted facts and time
 * at the right end. With `title` the left keeps one line, ends in an ellipsis
 * where it does not fit, and shows that full text on hover; `startRef` hands
 * the cut element out, for a caller that measures it.
 */
export function SummaryLine({
  start,
  end,
  title,
  startRef,
}: {
  start: ReactNode
  end: (string | undefined)[]
  title?: string
  startRef?: (element: HTMLElement | null) => void
}) {
  return (
    <span className="flex min-w-0 items-start gap-3">
      <span
        ref={startRef}
        title={title}
        className={cn("min-w-0 flex-1", title === undefined ? "break-words" : "truncate")}
      >
        {start}
      </span>
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
  role = "You",
}: {
  text: string
  queued?: boolean
  time?: string
  role?: string
}) {
  return (
    <div
      className={cn(
        "rounded-md px-2 py-1.5",
        role === "Developer" && "bg-muted/40 ring-1 ring-inset ring-border/70",
        role === "You" && "bg-muted/20",
      )}
    >
      <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold text-foreground">
        {role === "Developer" ? <CodeIcon aria-hidden className="size-3.5 shrink-0" /> : null}
        {role === "You" ? <UserIcon aria-hidden className="size-3.5 shrink-0" /> : null}
        {role}
      </div>
      <div className="min-w-0 flex-1">
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
  const icons = {
    message: MessageSquareIcon,
    thought: BrainIcon,
    plan: ListIcon,
    permission: ShieldIcon,
    note: InfoIcon,
    child: GitBranchIcon,
    error: TriangleAlertIcon,
    raw: FileTextIcon,
    prompt: TerminalIcon,
  } as const
  const Icon = block.type === "tool" || block.type === "question" ? null : icons[block.type]
  const decorated = !["question", "prompt", "message", "tool"].includes(block.type)
  const labels: Record<string, string> = {
    note: "Info",
    child: "Subagent",
    error: "Error",
    raw: "Event",
    plan: "Plan",
  }
  return (
    <div data-block={block.type} data-block-id={block.id} title={formatStamp(block.createdAt)}>
      {block.subagent ? (
        <div className="pb-0.5 font-mono text-[11px] text-muted-foreground">
          ↳ {block.subagent.title}
        </div>
      ) : null}
      <div
        className={cn(
          decorated && "flex items-start gap-2 px-2 py-1 text-xs leading-4",
          decorated && block.type !== "permission" && "ml-4",
          block.type === "tool" && "flex items-start gap-2",
        )}
      >
        {block.type === "tool" ? (
          <div className="mt-0.5">
            <ToolKind kind={block.tool.kind} />
          </div>
        ) : decorated && Icon ? (
          <Icon aria-hidden className="mt-px size-3.5 shrink-0 text-muted-foreground" />
        ) : null}
        <div className="min-w-0 flex-1">
          {labels[block.type] ? (
            <span className="mr-1 text-xs font-medium text-muted-foreground">
              {labels[block.type]}
            </span>
          ) : null}
          <BlockBody
            block={block}
            picker={picker}
            folded={folded}
            onToggle={() => onToggle(block.id, folded)}
          />
        </div>
      </div>
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
      return <PromptBox text={block.text} time={time} role={block.daemon ? "Developer" : "You"} />
    case "message":
      return (
        <div className="rounded-md bg-accent/30 px-2 py-1.5">
          <div className="mb-1 flex items-center gap-1.5 text-xs font-semibold">
            <BotIcon aria-hidden className="size-3.5 shrink-0" />
            Assistant
          </div>
          <Markdown>{block.text}</Markdown>
        </div>
      )
    case "thought":
      return (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          summary={<SummaryLine start={<>thought · {firstLine(block.text)}</>} end={[time]} />}
          className="[&>button]:p-0 [&>div]:px-0"
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
    case "tool":
      return <ToolDetails block={block} />
    case "permission": {
      const answer = block.answer
        ? block.answer.decidedBy
          ? `${block.answer.name} · decided by ${block.answer.decidedBy}`
          : block.answer.name
        : undefined
      const headline = `Permission · ${block.title}`
      const body = (
        <>
          {block.command ? (
            <CodeBlock code={block.command} language="shell" caption="command" className="mt-0.5" />
          ) : null}
          {block.diff ? (
            <CodeBlock code={block.diff} diff caption="diff" className="mt-0.5" />
          ) : null}
          {block.aiNote ? (
            <div className="pt-1 font-mono text-muted-foreground">{block.aiNote}</div>
          ) : null}
        </>
      )
      const frame = "text-xs"
      if (!answer) {
        return (
          <div className={cn(frame, "space-y-2")}>
            <div className="font-medium">
              <SummaryLine
                start={headline}
                title={`${headline}\n${formatStamp(block.createdAt)}`}
                end={[time]}
              />
            </div>
            {body}
            {picker}
          </div>
        )
      }
      const summary = (
        <SummaryLine
          start={headline}
          title={`${headline}\n${formatStamp(block.createdAt)}`}
          end={[answer, time]}
        />
      )
      if (!block.command && !block.diff && !block.aiNote) {
        return <div className={cn(frame, "px-2 py-1 font-mono")}>{summary}</div>
      }
      return (
        <Foldable folded={folded} onToggle={onToggle} summary={summary} className={frame}>
          {body}
        </Foldable>
      )
    }
    case "question":
      return picker
    case "note":
      return <span className="font-mono text-xs text-muted-foreground">{block.text}</span>
    case "child":
      return <div className="font-mono text-xs text-muted-foreground">{block.text}</div>
    case "error":
      return (
        <div className="px-2 py-1 text-xs text-status-danger-fg">
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

/** Permission activity opens its response controls in the shared details workspace. */
export const PermissionRow = memo(function PermissionRow({
  block,
  selected,
  onSelect,
}: {
  block: Extract<Block, { type: "permission" }>
  selected: boolean
  onSelect: (id: string, trigger: HTMLButtonElement) => void
}) {
  return (
    <div data-block="permission" data-block-id={block.id}>
      <button
        type="button"
        aria-pressed={selected}
        onClick={(event) => onSelect(block.id, event.currentTarget)}
        className={cn(
          "ml-4 flex w-[calc(100%-1rem)] min-w-0 items-center gap-2 rounded-md px-2 py-1 text-left text-xs outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50",
          selected && "bg-muted",
          !block.answer && "text-orange-500",
        )}
        title={`${block.title}\n${formatStamp(block.createdAt)}`}
      >
        <ShieldIcon aria-hidden className="size-3.5 shrink-0" />
        <span className="min-w-0 flex-1 truncate text-foreground">Permission · {block.title}</span>
        <span
          title={block.answer?.name}
          className="max-w-1/3 shrink-0 truncate text-muted-foreground"
        >
          {block.answer?.name ?? "Needs approval"}
        </span>
      </button>
    </div>
  )
})

/** One selectable activity row. Tool content lives in the details workspace. */
export const ToolRow = memo(function ToolRow({
  block,
  selected,
  onSelect,
}: {
  block: Extract<Block, { type: "tool" }>
  selected: boolean
  onSelect: (id: string, trigger: HTMLButtonElement) => void
}) {
  const { tool } = block
  const subject = tool.subject ?? tool.title
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    if (tool.endedAt || tool.status !== "in_progress") return
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [tool.endedAt, tool.status])
  return (
    <div data-block="tool" data-block-id={block.id}>
      <button
        type="button"
        aria-pressed={selected}
        onClick={(event) => onSelect(block.id, event.currentTarget)}
        title={`${subject}\n${formatStamp(block.createdAt)}`}
        className={cn(
          "flex w-[calc(100%-1rem)] ml-4 min-w-0 items-center gap-2 rounded-md px-2 py-1 text-left font-mono text-xs text-muted-foreground outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50",
          tool.status === "failed"
            ? "text-status-danger-fg"
            : tool.status === "completed"
              ? "text-status-done-fg"
              : "text-muted-foreground",
          selected && "bg-muted text-foreground",
        )}
      >
        <ToolKind kind={tool.kind} />
        <ToolStatus status={tool.status} />
        <span className="shrink-0 max-w-1/3 truncate font-medium text-foreground">
          {firstLine(tool.title)}
        </span>
        <span className="min-w-0 flex-1 truncate">
          {subject === tool.title ? firstLine(tool.output ?? "") : firstLine(subject)}
        </span>
        <span data-summary-end className="shrink-0 tabular-nums">
          {duration(
            tool.startedAt,
            tool.endedAt ??
              (tool.status === "in_progress" ? new Date(now).toISOString() : undefined),
          )}
        </span>
      </button>
    </div>
  )
})

/** Existing input, output and code changes, with shared highlighting and copy controls. */
function ToolDetails({ block }: { block: Extract<Block, { type: "tool" }> }) {
  const { tool } = block
  const subject = tool.subject ?? tool.title
  const input = tool.kind === "read" ? undefined : tool.inputText
  return (
    <div className="space-y-3 [&_pre]:leading-relaxed">
      <div className="flex items-center gap-2 font-mono text-xs text-muted-foreground">
        <ToolStatus status={tool.status} />
        <h4 className="min-w-0 flex-1 font-sans text-sm font-semibold text-foreground break-words whitespace-pre-wrap">
          {tool.title}
        </h4>
        <span className="shrink-0">{formatClock(block.createdAt)}</span>
      </div>
      {input?.includes(subject) ? null : (
        <p data-tool-subject className="font-mono text-xs break-words whitespace-pre-wrap">
          {subject}
        </p>
      )}
      {input ? <CodeBlock code={input} language={tool.inputLanguage} caption="input" /> : null}
      {tool.output ? (
        <CodeBlock
          code={tool.output}
          language={tool.outputLanguage}
          firstLine={tool.firstLine}
          caption="output"
        />
      ) : null}
      {tool.diff ? <CodeBlock code={tool.diff} diff caption="diff" /> : null}
      {!input && !tool.output && !tool.diff ? (
        <p className="text-xs text-muted-foreground">No input, output or code changes available.</p>
      ) : null}
    </div>
  )
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
