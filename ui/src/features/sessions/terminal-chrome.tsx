/**
 * What the terminal looks like around the bytes: its palette, the line above it
 * that says whether the log is live, and what the frame holds until the first
 * output lands.
 *
 * The emulator cannot inherit the app's palette — ANSI colours have to be real
 * colour values, and a pane's own escape sequences are meaningless without a
 * 16-colour set behind them. Background and foreground match the card the
 * terminal sits in so it does not read as a foreign rectangle.
 */

import type { ITheme } from "@xterm/xterm"
import { PlugZapIcon, SquareIcon } from "lucide-react"

import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/format"

import type { SessionLogStatus } from "./log-stream"

export const DARK_THEME: ITheme = {
  background: "#171717",
  foreground: "#e5e5e5",
  cursor: "#e5e5e5",
  cursorAccent: "#171717",
  selectionBackground: "#ffffff33",
  // The scrollbar is the only sign that there is history above the viewport,
  // so it is drawn stronger than xterm's default of 20% foreground.
  scrollbarSliderBackground: "#ffffff33",
  scrollbarSliderHoverBackground: "#ffffff55",
  scrollbarSliderActiveBackground: "#ffffff77",
  black: "#000000",
  red: "#cd3131",
  green: "#0dbc79",
  yellow: "#e5e510",
  blue: "#2472c8",
  magenta: "#bc3fbc",
  cyan: "#11a8cd",
  white: "#e5e5e5",
  brightBlack: "#7a7a7a",
  brightRed: "#f14c4c",
  brightGreen: "#23d18b",
  brightYellow: "#f5f543",
  brightBlue: "#3b8eea",
  brightMagenta: "#d670d6",
  brightCyan: "#29b8db",
  brightWhite: "#ffffff",
}

export const LIGHT_THEME: ITheme = {
  background: "#ffffff",
  foreground: "#0a0a0a",
  cursor: "#0a0a0a",
  cursorAccent: "#ffffff",
  selectionBackground: "#00000022",
  scrollbarSliderBackground: "#00000026",
  scrollbarSliderHoverBackground: "#00000044",
  scrollbarSliderActiveBackground: "#00000066",
  black: "#000000",
  red: "#cd3131",
  green: "#00a000",
  yellow: "#8a7500",
  blue: "#0451a5",
  magenta: "#bc05bc",
  cyan: "#0598bc",
  white: "#555555",
  brightBlack: "#6b6b6b",
  brightRed: "#cd3131",
  brightGreen: "#14ce14",
  brightYellow: "#997c00",
  brightBlue: "#0451a5",
  brightMagenta: "#bc05bc",
  brightCyan: "#0598bc",
  brightWhite: "#a5a5a5",
}

/**
 * Read off the class `next-themes` puts on `<html>` rather than from the hook:
 * the hook resolves a tick after mount, and the terminal would flash its own
 * default palette in between.
 */
export function currentTerminalTheme(): ITheme {
  return document.documentElement.classList.contains("dark") ? DARK_THEME : LIGHT_THEME
}

export function StreamStatus({
  status,
  error,
}: {
  status: SessionLogStatus
  error: string | null
}) {
  if (status === "ended") {
    return (
      <span className="flex items-center gap-1.5">
        <SquareIcon className="size-3" />
        Session ended — this is the full log, nothing more is coming.
      </span>
    )
  }
  if (status === "reconnecting") {
    return (
      <span className="flex items-center gap-1.5 text-destructive">
        <PlugZapIcon className="size-3" />
        Lost the log stream, reconnecting…
        {error ? <span className="text-muted-foreground">({error})</span> : null}
      </span>
    )
  }
  if (status === "connecting") {
    return (
      <span className="flex items-center gap-1.5">
        <span
          className="size-1.5 shrink-0 animate-pulse rounded-full bg-muted-foreground"
          aria-hidden
        />
        Connecting to the session's output…
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

/** Widths of the placeholder's lines. Uneven: terminal output is not prose. */
const PLACEHOLDER_LINES = [
  "w-1/3",
  "w-3/5",
  "w-2/5",
  "w-4/5",
  "w-1/2",
  "w-2/3",
  "w-1/4",
  "w-3/4",
  "w-2/5",
  "w-3/5",
  "w-1/2",
  "w-1/3",
]

/**
 * What the frame holds until the first snapshot lands. It covers the empty
 * emulator rather than standing in for it, so the frame already has the height
 * it will keep and nothing jumps when the output arrives.
 */
export function ConnectingScreen() {
  return (
    <div
      className="absolute inset-0 flex flex-col justify-end gap-2.5 overflow-hidden bg-card p-3"
      data-slot="terminal-connecting"
      aria-hidden
    >
      {PLACEHOLDER_LINES.map((width, index) => (
        <Skeleton
          key={width + String(index)}
          className={cn("h-2.5 shrink-0", width)}
          // Staggered, so it reads as output arriving rather than as one block
          // pulsing; the last lines are the newest and lead.
          style={{ animationDelay: `${(PLACEHOLDER_LINES.length - index) * 90}ms` }}
        />
      ))}
    </div>
  )
}
