import { CodeIcon, Loader2Icon, UserIcon } from "lucide-react"
import type { KeyboardEvent } from "react"

import { Markdown } from "@/components/markdown"
import { NamedIcon } from "@/components/named-icon"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/format"

import type { PermissionOption } from "./transcript"

/** A daemon prompt of more lines than this shows its first lines and a Show more. */
const FOLDED_LINES = 4

/**
 * What a person typed is a message: set apart and full size, with the time it
 * was sent. An icon names who typed it, with the role as its name and tooltip.
 */
export function PromptBox({
  text,
  queued,
  time,
  role = "You",
  fold,
}: {
  text: string
  queued?: boolean
  time?: string
  role?: string
  /** Folds a prompt of more than `FOLDED_LINES` lines; its state is the caller's. */
  fold?: { open: boolean; onToggle: () => void }
}) {
  const lines = text.split("\n")
  const folded = fold !== undefined && lines.length > FOLDED_LINES
  const shut = folded && !fold.open
  return (
    <div
      className={cn(
        "rounded-md px-2 py-1.5",
        role === "Developer" && "bg-muted/50 ring-1 ring-inset ring-border/60",
        role === "You" && "bg-primary/12 ring-1 ring-inset ring-border/50",
      )}
    >
      <NamedIcon
        icon={role === "Developer" ? CodeIcon : UserIcon}
        label={role}
        focusable
        className="mb-1 text-foreground"
      />
      <div className="min-w-0 flex-1">
        {queued ? (
          <span className="mb-1 inline-block rounded bg-muted px-1 font-mono text-[11px] text-muted-foreground">
            queued
          </span>
        ) : null}
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            {shut ? (
              <p className="whitespace-pre-wrap break-words text-sm">
                {lines.slice(0, FOLDED_LINES).join("\n")}
              </p>
            ) : (
              <Markdown>{text}</Markdown>
            )}
            {folded ? (
              <Button
                size="xs"
                variant="ghost"
                aria-expanded={fold.open}
                onClick={fold.onToggle}
                className="mt-1 font-mono text-muted-foreground"
              >
                {fold.open ? "Show less" : "Show more"}
              </Button>
            ) : null}
          </div>
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
