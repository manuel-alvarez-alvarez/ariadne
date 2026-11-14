import { Loader2Icon, PlugZapIcon } from "lucide-react"
import { type KeyboardEvent, useRef } from "react"

import type { SessionStatus } from "@/api"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn, formatTokens } from "@/lib/format"

import type { ConsoleStreamStatus } from "./console-stream"
import type { FailedSend } from "./console-view-state"
import { stateMark, type TokenTotals, type Transcript } from "./transcript"

/** Where the load of older events stands; `start` is the session's first event shown. */
export type History = { state: "idle" | "loading" | "start" } | { state: "error"; message: string }

export function FailedMessage({
  one,
  onRetry,
  onEdit,
}: {
  one: FailedSend
  onRetry: () => void
  onEdit: () => void
}) {
  return (
    <div
      role="alert"
      data-block="failed"
      className="space-y-1.5 rounded-md border border-status-danger/40 bg-status-danger-soft/40 px-3 py-2 text-xs"
    >
      <p className="text-status-danger-fg">{one.message}</p>
      <p className="whitespace-pre-wrap break-words text-sm">{one.text}</p>
      <div className="flex gap-1.5">
        <Button size="xs" variant="outline" onClick={onRetry}>
          Retry
        </Button>
        <Button size="xs" variant="outline" onClick={onEdit}>
          Edit
        </Button>
      </div>
    </div>
  )
}

export function HistoryLine({ history, onRetry }: { history: History; onRetry: () => void }) {
  if (history.state === "loading") {
    return (
      <div
        role="status"
        className="flex items-center gap-1.5 border-b px-3 py-1 font-mono text-xs text-muted-foreground"
      >
        <Loader2Icon className="size-3 animate-spin" aria-hidden />
        Loading older events…
      </div>
    )
  }
  if (history.state === "error") {
    return (
      <div
        role="alert"
        className="flex items-center gap-2 border-b px-3 py-1 font-mono text-xs text-status-danger-fg"
      >
        Could not load older events: {history.message}
        <Button size="xs" variant="outline" onClick={onRetry}>
          Retry
        </Button>
      </div>
    )
  }
  return null
}

/**
 * The views of a session with subagents: `main`, then each child with the
 * mark of its state. The bar is one stop in the Tab order: the arrow keys,
 * Home and End move between its views inside it.
 */
export function ViewBar({
  agents,
  shown,
  asking,
  onShow,
}: {
  agents: Transcript["subagents"]
  shown: string | undefined
  /** The child whose permission question waits. */
  asking: string | undefined
  onShow: (to: string | undefined, keepFocus?: boolean) => void
}) {
  const bar = useRef<HTMLDivElement>(null)
  const views = [
    { id: undefined, label: "main" },
    ...agents.map((one) => ({
      id: one.id,
      label: `${stateMark(one.state)} ${one.title}${one.id === asking ? " ?" : ""}`,
    })),
  ]
  const at = Math.max(
    views.findIndex((one) => one.id === shown),
    0,
  )

  function onKeyDown(event: KeyboardEvent) {
    const last = views.length - 1
    const to =
      event.key === "ArrowRight"
        ? (at + 1) % views.length
        : event.key === "ArrowLeft"
          ? (at + last) % views.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : undefined
    if (to === undefined) return
    event.preventDefault()
    onShow(views[to]?.id, true)
    bar.current?.querySelectorAll<HTMLElement>('[role="tab"]')[to]?.focus()
  }

  return (
    <div
      ref={bar}
      role="tablist"
      aria-label="Console views"
      onKeyDown={onKeyDown}
      className="flex flex-wrap gap-1 border-t px-2 py-1 font-mono text-xs"
    >
      {views.map((one, index) => (
        <button
          key={one.id ?? ""}
          type="button"
          role="tab"
          aria-selected={one.id === shown}
          tabIndex={index === at ? 0 : -1}
          onClick={() => onShow(one.id)}
          className={cn(
            "rounded px-1.5 py-0.5 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
            one.id === shown
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:bg-muted",
          )}
        >
          {one.label}
        </button>
      ))}
    </div>
  )
}

export function elapsedSeconds(since: string | undefined, now: number): number | undefined {
  if (since === undefined) return undefined
  const started = Date.parse(since)
  return Number.isNaN(started) ? undefined : Math.max(0, Math.floor((now - started) / 1000))
}

/** `12s`, or `1m 04s` past a minute: the CLI's turn clock. */
function clock(seconds: number): string {
  if (seconds < 60) return `${seconds}s`
  return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`
}

export function StatusRow({
  status,
  turn,
  tokens,
}: {
  status: SessionStatus
  turn?: { tool?: string; seconds?: number }
  tokens: TokenTotals
}) {
  const firstLine = turn?.tool?.split("\n")[0] ?? ""
  const spent = tokens.input > 0 || tokens.output > 0
  return (
    <section
      aria-label="Session status"
      className="flex items-center gap-3 whitespace-nowrap border-t px-3 py-1 font-mono text-xs text-muted-foreground"
    >
      <span className="shrink-0">{status}</span>
      {turn ? (
        <span className="flex min-w-0 items-center gap-1.5">
          <Loader2Icon className="size-3 shrink-0 animate-spin" aria-hidden />
          {turn.tool === undefined ? (
            "thinking"
          ) : (
            <Tooltip>
              <TooltipTrigger render={<span className="min-w-0 truncate" />}>
                running {firstLine}
              </TooltipTrigger>
              <TooltipContent className="break-words">{turn.tool}</TooltipContent>
            </Tooltip>
          )}
          {turn.seconds === undefined ? null : (
            <span className="shrink-0">{clock(turn.seconds)}</span>
          )}
        </span>
      ) : null}
      {spent ? (
        <span className="ml-auto shrink-0">
          ↑{formatTokens(tokens.input)} ↓{formatTokens(tokens.output)}
        </span>
      ) : null}
    </section>
  )
}

export function StreamStatusLine({
  status,
  error,
}: {
  status: ConsoleStreamStatus
  error: string | null
}) {
  if (status === "reconnecting") {
    return (
      <span className="flex items-center gap-1.5 text-status-danger-fg">
        <PlugZapIcon className="size-3" />
        Lost the console, reconnecting…
        {error ? <span className="text-muted-foreground">({error})</span> : null}
      </span>
    )
  }
  if (status === "connecting") {
    return (
      <span className="flex items-center gap-1.5">
        <Loader2Icon className="size-3 animate-spin" aria-hidden />
        Connecting to the session's console…
      </span>
    )
  }
  return (
    <span className="flex items-center gap-1.5">
      <span className="size-1.5 shrink-0 animate-pulse rounded-full bg-status-done" aria-hidden />
      Live
    </span>
  )
}
