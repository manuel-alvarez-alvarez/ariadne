import { Loader2Icon } from "lucide-react"
import type { KeyboardEvent, ReactNode } from "react"

import { CodeBlock } from "@/components/code-block"
import { Markdown } from "@/components/markdown"
import { cn } from "@/lib/format"
import { languageForPath } from "@/lib/highlight"

import { Foldable } from "./foldable"
import { type Block, isTerminal, type PermissionOption } from "./transcript"

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

/** What a person typed is a message: set apart and full size. */
export function PromptBox({ text, queued }: { text: string; queued?: boolean }) {
  return (
    <div className="rounded-md border-l-2 border-primary bg-primary/10 px-3 py-2">
      {queued ? (
        <span className="mb-1 inline-block rounded bg-muted px-1 font-mono text-[11px] text-muted-foreground">
          queued
        </span>
      ) : null}
      <p className="whitespace-pre-wrap break-words">{text}</p>
    </div>
  )
}

export function BlockView({
  block,
  picker,
  folded,
  onToggle,
}: {
  block: Block
  picker: ReactNode
  folded: boolean
  onToggle: (id: string, folded: boolean) => void
}) {
  return (
    <div data-block={block.type} data-block-id={block.id}>
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
  switch (block.type) {
    case "prompt":
      return block.daemon ? (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          className="rounded-md border-l-2 bg-muted/40"
          summary={
            <>
              daemon prompt · {firstLine(block.text)} · {lineCount(block.text)}
            </>
          }
        >
          <p className="whitespace-pre-wrap break-words">{block.text}</p>
        </Foldable>
      ) : (
        <PromptBox text={block.text} />
      )
    case "message":
      return <Markdown>{block.text}</Markdown>
    case "thought":
      return (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          summary={<>thought · {firstLine(block.text)}</>}
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
      const body = tool.output || tool.diff
      const summary = (
        <>
          <ToolStatus status={tool.status} />
          {tool.kind ? ` · ${tool.kind}` : null}
          {" · "}
          <span className="font-medium text-foreground">{tool.title}</span>
          {body ? ` · ${lineCount(body)}` : null}
        </>
      )
      if (!body) {
        return (
          <div className="rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
            {summary}
          </div>
        )
      }
      // A file the tool read is shown in the file's language.
      const language =
        tool.kind === "read" ? languageForPath(tool.title.split(/\s+/).at(-1) ?? "") : undefined
      return (
        <Foldable
          folded={folded}
          onToggle={onToggle}
          summary={summary}
          className="rounded-md border"
        >
          {tool.output ? <Output text={tool.output} language={language} /> : null}
          {tool.diff ? <Diff text={tool.diff} /> : null}
        </Foldable>
      )
    }
    case "permission":
      return (
        <div className="rounded-md border border-status-warn/40 bg-status-warn-soft/30 px-3 py-2 text-xs">
          <div className="font-medium">Permission · {block.title}</div>
          {block.command ? <Output text={block.command} /> : null}
          {block.diff ? <Diff text={block.diff} /> : null}
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

function ToolStatus({ status }: { status?: string }) {
  const tone =
    status === "failed"
      ? "text-status-danger-fg"
      : isTerminal(status)
        ? "text-status-done-fg"
        : "text-status-active-fg"
  return <span className={tone}>{status ?? "pending"}</span>
}

/** A tool's output or a command, whole: it grows with its content and wraps, and never scrolls inside. */
function Output({ text, language }: { text: string; language?: string }) {
  return <CodeBlock code={text} language={language} className="mt-1.5" />
}

/** A file change, with its added and removed lines colored. */
function Diff({ text }: { text: string }) {
  return <CodeBlock code={text} diff className="mt-1.5" />
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
