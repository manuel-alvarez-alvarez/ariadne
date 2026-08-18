/**
 * The agent's tmux pane, live, in xterm.js.
 *
 * The daemon sends raw terminal bytes — escape sequences, cursor addressing,
 * colours and all — so the output is written into a real terminal emulator
 * rather than rendered as text: anything less would show the control codes of
 * a full-screen TUI instead of what the agent is actually drawing.
 *
 * The emulator runs at the pane's grid, never at the panel's. A pane is 80×24,
 * or whatever a client last attached with, and its TUI addresses the cursor
 * and erases lines in *that* grid: rendered even a column wider, every line
 * that wraps in the pane but not here shifts the rows below it, and each
 * repaint lands on the wrong one. So the frame's width picks the font size
 * instead of the column count — a narrower panel shows the same pane, smaller
 * — and the grid changes only when the daemon says the pane changed (see
 * `log-stream.ts`).
 *
 * Typing goes the other way, to `POST /v1/sessions/{id}/input`, and only for a
 * live session — a finished one has no pane to type into, so its terminal
 * stays display-only rather than swallowing keystrokes. Nothing is echoed
 * locally: what appears on screen is what tmux sent back through the log
 * stream, exactly as it would in a real attach.
 *
 * A panel is a small window onto all of that, so the terminal can be lifted
 * into a near-fullscreen dialog, the way `task-diff.tsx` lifts the diff. Only
 * the frame changes: the grid stays the pane's there too, and what the room
 * buys is a bigger font — which is why the height budget and the font ceiling
 * are per-frame (see {@link screenHeightBudget}) rather than the panel's
 * constants everywhere.
 */

import { FitAddon } from "@xterm/addon-fit"
import { type ITheme, Terminal } from "@xterm/xterm"
import "@xterm/xterm/css/xterm.css"
import {
  ArrowDownToLineIcon,
  KeyboardIcon,
  Maximize2Icon,
  Minimize2Icon,
  PlugZapIcon,
  RotateCwIcon,
  SquareIcon,
} from "lucide-react"
import { useTheme } from "next-themes"
import { useCallback, useEffect, useRef, useState } from "react"
import { toast } from "sonner"

import type { SessionStatus } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import { Skeleton } from "@/components/ui/skeleton"
import { describeError } from "@/lib/errors"
import { cn } from "@/lib/utils"
import { useBaseUrl } from "@/stores/settings"

import {
  type PaneSize,
  type SessionLogStatus,
  SessionLogStream,
  sessionLogStreamUrl,
} from "./log-stream"
import { sendSessionInput } from "./queries"
import { isLiveStatus } from "./session-display"
import { writeDelta, writeResize, writeSnapshot } from "./terminal-sink"

/** Lines kept above the viewport. A busy agent fills a pane fast. */
const SCROLLBACK = 5_000

/**
 * What to draw at until the daemon says otherwise: tmux's own default size,
 * which is what these panes are created at. A session that is already over
 * has no pane left to measure, and its console log was written at whatever
 * size the pane had then.
 */
const DEFAULT_PANE_SIZE: PaneSize = { cols: 80, rows: 24 }

/**
 * Font sizes the pane may be scaled to. Below the lower bound a monospace
 * grid stops being readable, so a pane too wide for the frame overflows it
 * and scrolls sideways instead of shrinking into illegibility.
 */
const MIN_FONT_SIZE = 8
const MAX_FONT_SIZE = 15
/**
 * The ceiling in the expanded frame, where the pane is the whole screen and
 * not one card among others. It is high enough that the room — the dialog's
 * height, or its width for a wide pane — is what stops the font on any usual
 * grid, rather than the ceiling standing in for the panel's.
 */
const EXPANDED_MAX_FONT_SIZE = 24
/** Where scaling starts, and what a pane is drawn at when it fits as it is. */
const BASE_FONT_SIZE = 12
const LINE_HEIGHT = 1.2
/** Tallest the grid may get before the font shrinks to fit it in (`28rem`). */
const MAX_SCREEN_HEIGHT = 448

/**
 * The terminal cannot inherit the app's palette: ANSI colours have to be real
 * colour values, and a pane's own escape sequences are meaningless without a
 * 16-colour set behind them. Background and foreground match the card the
 * terminal sits in so it does not read as a foreign rectangle.
 */
const DARK_THEME: ITheme = {
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

const LIGHT_THEME: ITheme = {
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

function currentTerminalTheme(): ITheme {
  return document.documentElement.classList.contains("dark") ? DARK_THEME : LIGHT_THEME
}

export function SessionTerminal({
  sessionId,
  status,
  className,
  screenClassName,
}: {
  sessionId: string
  /** Whether the session can still be typed into; see {@link isLiveStatus}. */
  status: SessionStatus
  className?: string
  /** Classes for the frame the emulator draws in. Merged over the default. */
  screenClassName?: string
}) {
  const [expanded, setExpanded] = useState(false)
  /**
   * Whether the emulator holds the keyboard. Read while handling Escape, and
   * only then, so it is a ref: a re-render per focus change would buy nothing
   * and cost the terminal a repaint.
   */
  const focused = useRef(false)

  const view = (
    <TerminalView
      sessionId={sessionId}
      status={status}
      className={className}
      screenClassName={screenClassName}
      expanded={expanded}
      onExpandedChange={setExpanded}
      onFocusChange={(next) => {
        focused.current = next
      }}
    />
  )

  // Moving between the two frames remounts the emulator, and with it the log
  // stream. That is not a loss: every connection opens with a full snapshot,
  // so the pane is redrawn as it is now rather than as it was when the panel
  // first attached.
  if (!expanded) return view

  return (
    <>
      {/* The panel keeps its place in the card rather than collapsing behind
          the dialog, and says where the terminal went. */}
      <EmptyState
        emphasis="quiet"
        title="The terminal is open in the expanded view"
        action={
          <Button variant="outline" size="sm" onClick={() => setExpanded(false)}>
            <Minimize2Icon />
            Back to the panel
          </Button>
        }
      />
      <Dialog
        open
        onOpenChange={(open, details) => {
          // Escape is a keystroke the agent's TUI wants — it is `\x1b` on the
          // way to the pane — so a focused terminal keeps it and the dialog is
          // left to its own collapse control. The panel behind is a dialog of
          // its own, and Base UI already holds a nested Escape back from it,
          // so nothing else closes on the same press either.
          if (details.reason === "escape-key" && focused.current) {
            details.cancel()
            return
          }
          if (!open) setExpanded(false)
        }}
      >
        {/* Sized like the expanded diff, and a single stretched row so the
            status line stays above a screen that takes the rest. */}
        <DialogContent
          showCloseButton={false}
          className="h-[calc(100dvh-2rem)] w-[calc(100vw-2rem)] max-w-[calc(100vw-2rem)] grid-rows-[minmax(0,1fr)] sm:max-w-[calc(100vw-2rem)]"
        >
          <DialogTitle className="sr-only">Terminal of the session</DialogTitle>
          {view}
        </DialogContent>
      </Dialog>
    </>
  )
}

/**
 * The terminal itself — the status line, and the frame the emulator draws in
 * — as it renders in one of the two places it can be: the panel it belongs to,
 * or the dialog it was expanded into.
 */
function TerminalView({
  sessionId,
  status,
  className,
  screenClassName,
  expanded,
  onExpandedChange,
  onFocusChange,
}: {
  sessionId: string
  status: SessionStatus
  className?: string
  screenClassName?: string
  /** Whether this is the expanded view rather than the panel's. */
  expanded: boolean
  onExpandedChange: (expanded: boolean) => void
  onFocusChange: (focused: boolean) => void
}) {
  const live = isLiveStatus(status)
  const baseUrl = useBaseUrl()
  const { resolvedTheme } = useTheme()
  const frameRef = useRef<HTMLDivElement | null>(null)
  const containerRef = useRef<HTMLDivElement | null>(null)
  const terminalRef = useRef<Terminal | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const streamRef = useRef<SessionLogStream | null>(null)
  const [streamStatus, setStreamStatus] = useState<SessionLogStatus>("connecting")
  const [error, setError] = useState<string | null>(null)
  /** Whether anything has been drawn yet; until then the frame is a placeholder. */
  const [attached, setAttached] = useState(false)
  /** Whether the viewport is on the newest output rather than up in the history. */
  const [following, setFollowing] = useState(true)
  // Held in a ref so the emulator's own listeners can reach the current
  // callback without the emulator being torn down and rebuilt for it.
  const focusChangeRef = useRef(onFocusChange)
  useEffect(() => {
    focusChangeRef.current = onFocusChange
  }, [onFocusChange])

  /**
   * Fit the pane's grid into the frame by scaling the font — the one dimension
   * that is ours to choose.
   *
   * Both factors are measured rather than derived: `proposeDimensions` says
   * how many columns the current font gets out of the frame, and the screen's
   * own height says what a row costs, neither of which follows from the font
   * size alone. The ratio to the grid we want is the factor the font is off
   * by, and a pass or two settles it — the size is quantised, so the first
   * answer is rarely exact.
   *
   * What the grid may take, and how large the font may get doing it, are the
   * frame's to say: the panel caps both so the terminal stays one card among
   * others, and the expanded frame has a dialog's height to fill.
   */
  const scaleToFit = useCallback(() => {
    const terminal = terminalRef.current
    const fit = fitRef.current
    const container = containerRef.current
    if (!terminal || !fit || !container) return
    const budget = screenHeightBudget(expanded ? frameRef.current : null, container)
    const ceiling = expanded ? EXPANDED_MAX_FONT_SIZE : MAX_FONT_SIZE
    for (let pass = 0; pass < 4; pass++) {
      const proposed = fit.proposeDimensions()
      if (!proposed || !Number.isFinite(proposed.cols)) return
      const current = terminal.options.fontSize ?? BASE_FONT_SIZE
      const height = container.querySelector<HTMLElement>(".xterm-screen")?.clientHeight ?? 0
      // Whichever runs out first: the width there is, or the height the grid
      // may take before it is worth showing smaller.
      const scale = Math.min(
        proposed.cols / terminal.cols,
        height > 0 ? budget / height : Number.POSITIVE_INFINITY,
      )
      const next = clamp(quantise(current * scale), MIN_FONT_SIZE, ceiling)
      if (next === current) return
      terminal.options.fontSize = next
    }
  }, [expanded])

  // The emulator itself: created once, and kept across daemon-URL changes so a
  // reconnect does not flash an empty box.
  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    const terminal = new Terminal({
      // tmux's captured pane ends its lines with a bare newline; without this
      // every line would start where the previous one stopped.
      convertEol: true,
      // Never fitted to the box: the stream is what says how big the grid is.
      cols: DEFAULT_PANE_SIZE.cols,
      rows: DEFAULT_PANE_SIZE.rows,
      // Turned on by the input effect below once the session is known to be
      // live; a terminal that cannot reach a pane must not pretend to.
      disableStdin: true,
      cursorBlink: false,
      cursorInactiveStyle: "none",
      scrollback: SCROLLBACK,
      fontFamily: "'Geist Mono Variable', ui-monospace, monospace",
      fontSize: BASE_FONT_SIZE,
      lineHeight: LINE_HEIGHT,
      allowTransparency: false,
      // Read off the class `next-themes` puts on <html> rather than from the
      // hook: the hook resolves a tick after mount, and the terminal would
      // flash its own default palette in between.
      theme: currentTerminalTheme(),
    })
    // Loaded for its measurements alone — see `scaleToFit`. The grid stays the
    // pane's, so `fit()` itself is never called.
    const fit = new FitAddon()
    terminal.loadAddon(fit)
    terminal.open(container)
    terminalRef.current = terminal
    fitRef.current = fit

    // Scrolled up into the history, the viewer stops seeing what the agent is
    // doing now; the frame offers a way back rather than leaving them there.
    const scrolled = terminal.onScroll(() => {
      const buffer = terminal.buffer.active
      setFollowing(buffer.viewportY >= buffer.baseY)
    })

    // The grid arrives in the stream and applies whenever the parser reaches
    // it, so the emulator itself — not the message that caused it — is what
    // says a new one is in effect and the font has to be scaled to it.
    const resized = terminal.onResize(() => scaleToFit())

    // xterm focuses itself when its own screen is clicked; this covers the
    // padding around it, so the whole box is somewhere to start typing.
    // Keyboard users need no equivalent — xterm's textarea is in the tab
    // order — which is why it is a DOM listener and not an `onClick` prop.
    const focusTerminal = () => terminal.focus()
    container.addEventListener("click", focusTerminal)

    // Whether the keyboard is the pane's decides who gets Escape in the
    // expanded view — the agent's TUI, or the dialog. It is watched on the
    // container rather than on xterm's textarea because that is the element
    // this effect owns, and `focusin`/`focusout` bubble to it from wherever
    // inside the emulator focus actually lands.
    const focusIn = () => focusChangeRef.current(true)
    const focusOut = () => focusChangeRef.current(false)
    container.addEventListener("focusin", focusIn)
    container.addEventListener("focusout", focusOut)

    return () => {
      container.removeEventListener("click", focusTerminal)
      container.removeEventListener("focusin", focusIn)
      container.removeEventListener("focusout", focusOut)
      focusChangeRef.current(false)
      scrolled.dispose()
      resized.dispose()
      terminalRef.current = null
      fitRef.current = null
      terminal.dispose()
    }
  }, [scaleToFit])

  // Resizing the window, the panel or the sheet it sits in re-scales the same
  // grid to what the frame now is.
  useEffect(() => {
    const frame = frameRef.current
    if (!frame) return
    scaleToFit()
    let width = frame.clientWidth
    let height = frame.clientHeight
    const observer = new ResizeObserver(() => {
      // In the panel the frame's height follows the terminal's, which follows
      // the font, so reacting to it would be a loop: only a new width is a new
      // fit there. The expanded frame is given its height by the dialog and
      // keeps it whatever the font does, so a window that only got taller is a
      // new fit too.
      const resized = frame.clientWidth !== width || (expanded && frame.clientHeight !== height)
      if (!resized) return
      width = frame.clientWidth
      height = frame.clientHeight
      scaleToFit()
    })
    observer.observe(frame)
    return () => observer.disconnect()
  }, [expanded, scaleToFit])

  useEffect(() => {
    const terminal = terminalRef.current
    if (!terminal) return
    terminal.options.theme = resolvedTheme === "dark" ? DARK_THEME : LIGHT_THEME
  }, [resolvedTheme])

  // Keystrokes to the pane. `onData` hands over what a real terminal would
  // have written to the pty — `\r` for Return, `\x03` for Ctrl-C, `\x1b[A` for
  // Up — which the daemon forwards to tmux verbatim, so it is sent as-is and
  // never echoed here: the echo arrives with the rest of the pane's output.
  //
  // Re-attached whenever the session's liveness changes, so a session that
  // ends while it is on screen stops accepting input on the spot.
  useEffect(() => {
    const terminal = terminalRef.current
    if (!terminal) return
    terminal.options.disableStdin = !live
    if (!live) return
    const input = terminal.onData((data) => {
      sendSessionInput(sessionId, data).catch((error: unknown) => {
        // One toast per session, not per keystroke: whatever refuses the
        // input refuses all of it, and a burst of typing would otherwise
        // bury the screen in identical toasts.
        toast.error("Could not send that keystroke", {
          id: `session-input-${sessionId}`,
          description: describeError(error),
        })
      })
    })
    return () => input.dispose()
  }, [live, sessionId])

  // One stream per session and daemon. Every connection opens with a full
  // snapshot, so a reconnect replaces the contents instead of appending to
  // them — splicing a fresh capture onto stale output would invent a history
  // the agent never printed. See `terminal-sink.ts` for why "replaces" is a
  // write and not a `reset()` call, and why the grid is a write too.
  useEffect(() => {
    const stream = new SessionLogStream(sessionLogStreamUrl(baseUrl, sessionId), {
      onResize: (size) => {
        const terminal = terminalRef.current
        if (terminal) writeResize(terminal, size.cols, size.rows)
      },
      onSnapshot: (chunk) => {
        const terminal = terminalRef.current
        if (terminal) writeSnapshot(terminal, chunk)
        setAttached(true)
      },
      onDelta: (chunk) => {
        const terminal = terminalRef.current
        if (terminal) writeDelta(terminal, chunk)
      },
      onEnd: () => {},
      onStatus: (next, why) => {
        setStreamStatus(next)
        if (why !== undefined) setError(why)
      },
    })
    streamRef.current = stream
    stream.start()
    return () => {
      streamRef.current = null
      stream.stop()
    }
  }, [baseUrl, sessionId])

  return (
    <div className={cn("flex min-h-0 flex-col gap-2", expanded && "h-full", className)}>
      <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
        {/* Announced: this line changes on its own — the stream dropping and
            coming back is the daemon's doing, not the user's — and it is the
            only place on screen that says the log went stale. */}
        <span role="status" className="min-w-0">
          <StreamStatus status={streamStatus} error={error} />
        </span>
        <div className="flex shrink-0 items-center gap-1">
          {streamStatus === "ended" ? (
            <Button variant="ghost" size="xs" onClick={() => streamRef.current?.restart()}>
              <RotateCwIcon />
              Reload
            </Button>
          ) : live ? (
            <span className="flex items-center gap-1.5">
              <KeyboardIcon className="size-3" />
              Click to type into the agent's terminal
            </span>
          ) : null}
          <Button
            variant="ghost"
            size="icon-xs"
            onClick={() => onExpandedChange(!expanded)}
            aria-pressed={expanded}
            aria-label={
              expanded ? "Collapse the terminal back into the panel" : "Expand the terminal"
            }
            title={expanded ? "Collapse the terminal back into the panel" : "Expand the terminal"}
          >
            {expanded ? <Minimize2Icon /> : <Maximize2Icon />}
          </Button>
        </div>
      </div>
      {/*
        The frame is what makes the pane read as its own scrolling region:
        wheeling over it moves the pane's history and not the panel behind it,
        which is only ever obvious if the region visibly is one.
      */}
      <div
        ref={frameRef}
        className={cn(
          "relative overflow-hidden rounded-lg border bg-card shadow-xs",
          // All that is left of the dialog, and the height `scaleToFit`
          // measures the grid against there.
          expanded && "min-h-0 flex-1",
          screenClassName,
        )}
      >
        {/*
          Full width even when the grid is narrower, because it is the width
          `scaleToFit` measures to pick the font — and it is the frame's, not
          the terminal's own. Sideways scrolling is the last resort for a pane
          too wide to shrink into (see MIN_FONT_SIZE).
        */}
        <div ref={containerRef} className="w-full overflow-x-auto p-2" />
        {attached ? null : <ConnectingScreen />}
        {attached && !following ? (
          <Button
            variant="secondary"
            size="xs"
            className="absolute right-3 bottom-3 shadow-sm"
            onClick={() => terminalRef.current?.scrollToBottom()}
          >
            <ArrowDownToLineIcon />
            Jump to latest
          </Button>
        ) : null}
      </div>
    </div>
  )
}

/**
 * What the frame holds until the first snapshot lands. It covers the empty
 * emulator rather than standing in for it, so the frame already has the height
 * it will keep and nothing jumps when the output arrives.
 */
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

function ConnectingScreen() {
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

function StreamStatus({ status, error }: { status: SessionLogStatus; error: string | null }) {
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

/**
 * How tall the grid may get in this frame before the font shrinks to fit it in.
 *
 * In the panel that is a fixed {@link MAX_SCREEN_HEIGHT}: the frame grows with
 * the terminal there, so nothing but a cap says when the pane has taken enough
 * of the card. A frame that was given its height — the expanded dialog's —
 * measures instead, and the answer is what is left of it once the padding the
 * emulator draws inside is out of the way.
 */
function screenHeightBudget(frame: HTMLElement | null, container: HTMLElement): number {
  if (!frame) return MAX_SCREEN_HEIGHT
  const style = getComputedStyle(container)
  const padding =
    (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.paddingBottom) || 0)
  return Math.max(0, frame.clientHeight - padding)
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}

/**
 * Down to the nearest half pixel. Rounding up would overflow the frame the
 * size was measured against, and whole pixels alone would leave a visible
 * margin at the small sizes a wide pane needs.
 */
function quantise(fontSize: number): number {
  return Math.floor(fontSize * 2) / 2
}
