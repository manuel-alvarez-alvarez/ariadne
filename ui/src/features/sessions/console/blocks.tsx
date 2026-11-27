import { CodeIcon, Loader2Icon, UserIcon } from "lucide-react"
import type { KeyboardEvent } from "react"

import { Markdown } from "@/components/markdown"
import { cn } from "@/lib/format"

import type { PermissionOption } from "./transcript"

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
        role === "Developer" && "bg-muted/50 ring-1 ring-inset ring-border/60",
        role === "You" && "bg-primary/12 ring-1 ring-inset ring-border/50",
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
