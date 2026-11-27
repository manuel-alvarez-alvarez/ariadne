import { CircleHelpIcon, Loader2Icon, PencilIcon, PlugZapIcon, RotateCcwIcon } from "lucide-react"
import type { ReactNode } from "react"

import type { SessionStatus } from "@/api"
import { IconButton } from "@/components/icon-button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { formatTokens } from "@/lib/format"

import type { ConsoleStreamStatus } from "../trajectory/trajectory-stream"
import type { FailedSend } from "./console-view-state"
import type { TokenTotals } from "./transcript"

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
        <IconButton size="icon-xs" icon={<RotateCcwIcon />} label="Retry" onClick={onRetry} />
        <IconButton size="icon-xs" icon={<PencilIcon />} label="Edit" onClick={onEdit} />
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
        <IconButton size="icon-xs" icon={<RotateCcwIcon />} label="Retry" onClick={onRetry} />
      </div>
    )
  }
  return null
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
  waiting = false,
  tokens,
  facts,
}: {
  status: SessionStatus
  turn?: { tool?: string; seconds?: number }
  /** A question of the agent waits for the user. */
  waiting?: boolean
  tokens: TokenTotals
  /** More of the session's state, before the token totals. They give way first on a narrow row. */
  facts?: ReactNode
}) {
  const firstLine = turn?.tool?.split("\n")[0] ?? ""
  const spent = tokens.input > 0 || tokens.output > 0
  return (
    <section
      aria-label="Session status"
      className="flex items-center gap-3 overflow-hidden whitespace-nowrap border-t px-3 py-1 font-mono text-xs text-muted-foreground"
    >
      <span className="shrink-0">{status}</span>
      {waiting ? (
        <span className="flex min-w-0 items-center gap-1.5">
          <CircleHelpIcon className="size-3 shrink-0" aria-hidden />
          waiting for you
        </span>
      ) : turn ? (
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
      {facts}
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
