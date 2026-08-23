/**
 * The agent's tmux pane, live, in xterm.js.
 *
 * The daemon sends raw terminal bytes — escape sequences, cursor addressing,
 * colours and all — so the output is written into a real terminal emulator
 * rather than rendered as text: anything less would show the control codes of
 * a full-screen TUI instead of what the agent is actually drawing.
 *
 * The emulator runs at the pane's grid, never at the panel's. A pane's TUI
 * addresses the cursor and erases lines in *that* grid: rendered even a column
 * wider, every line that wraps in the pane but not here shifts the rows below
 * it, and each repaint lands on the wrong one. So the grid is never fitted to
 * the frame here — it changes when the daemon says the pane changed, and only
 * then (see `log-stream.ts`).
 *
 * What the frame does instead is ask for the pane it wants. A tmux pane is
 * 80×24 until a client attaches to it and a browser never does, so a live
 * session measures the room it has at {@link BASE_FONT_SIZE} and posts that
 * grid to `POST /v1/sessions/{id}/resize` — the attach it cannot make. The
 * pane redraws itself at the new size and the stream reports it, so what the
 * panel ends up showing is a pane the size of the panel rather than a small
 * one blown up. Scaling the font (see {@link scaleToFit}) is what is left for
 * the cases that cannot: a session that is over, whose replay was written at
 * the size its pane had then, and the moment between asking for a grid and
 * being given it.
 *
 * Typing goes the other way, to `POST /v1/sessions/{id}/input`, and only for a
 * live session — a finished one has no pane to type into, so its terminal
 * stays display-only rather than swallowing keystrokes. Nothing is echoed
 * locally: what appears on screen is what tmux sent back through the log
 * stream, exactly as it would in a real attach.
 *
 * A panel is a small window onto all of that, so the terminal can be lifted
 * into a near-fullscreen dialog, the way `task-diff.tsx` lifts the diff. Only
 * the frame changes: the pane is asked for the dialog's room on the way in and
 * for the panel's on the way out, and a replay that can only be scaled gets a
 * bigger font — which is why the height budget and the font ceiling are
 * per-frame (see {@link screenHeightBudget}) rather than the panel's constants
 * everywhere. The emulator itself makes that trip: it is rendered
 * into an element of its own that is *moved* between the two frames, so
 * expanding costs a re-scale and nothing more — the same emulator, the same
 * open stream, and no snapshot fetched again for output already on screen.
 *
 * Drawing goes through the GPU where there is one: `@xterm/addon-webgl` is
 * fetched once the emulator is open and loaded into it, and the DOM renderer
 * xterm starts with is what stays when it cannot be — no WebGL2 in this
 * browser, no addon delivered, or a context the driver takes back later.
 */

import { FitAddon } from "@xterm/addon-fit"
import type { WebglAddon } from "@xterm/addon-webgl"
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
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react"
import { createPortal } from "react-dom"
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
import { paneFit, sameSize } from "./pane-fit"
import { sendSessionInput, sendSessionResize } from "./queries"
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
 * How long a frame has to stop moving before the pane is asked to match it.
 *
 * Dragging a panel's edge is a stream of sizes and every one of them would be
 * a `resize-window` and a full repaint of the pane; what the pane is asked for
 * is where the drag came to rest.
 */
const RESIZE_DEBOUNCE_MS = 200

/**
 * How many times `scaleToFit` may measure and correct itself. Each pass is a
 * reflow, and a pass that measures exactly what the last one did stops on its
 * own, so this is only the ceiling for a frame that keeps moving under it.
 */
const MAX_SCALE_PASSES = 4

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
  /**
   * The element the terminal is rendered into, made once and kept for as long
   * as this component is on screen.
   *
   * The panel and the expanded dialog are two different places in the tree, so
   * a terminal rendered in both is a different component in each: expanding
   * would build a new emulator, open a new connection, and fetch a whole
   * snapshot for output that is already on screen. Rendered through a portal
   * into an element that is *appended* to whichever frame is showing, the move
   * is a DOM move instead — the same nodes, the same React state, the same
   * stream, in a different box.
   */
  const [host] = useState(createTerminalHost)

  /** Park the terminal in the frame that is showing it. */
  const anchor = useCallback(
    (node: HTMLDivElement | null) => {
      // Appending an element that has a parent already moves it, subtree and
      // all. Refs run once the new frame is in the document, so the emulator
      // is never re-parented into a node that is not.
      node?.append(host)
    },
    [host],
  )

  const view = createPortal(
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
    />,
    host,
  )

  if (!expanded) {
    return (
      <>
        {view}
        <div ref={anchor} className="contents" />
      </>
    )
  }

  return (
    <>
      {view}
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
          <div ref={anchor} className="contents" />
        </DialogContent>
      </Dialog>
    </>
  )
}

/**
 * The element {@link SessionTerminal} renders the terminal into and moves
 * between its two frames.
 *
 * `display: contents` keeps it out of the layout it is dropped into: what the
 * frame lays out is the terminal itself, exactly as if it were the child it
 * would otherwise have been.
 */
function createTerminalHost(): HTMLDivElement {
  const host = document.createElement("div")
  host.style.display = "contents"
  return host
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
  /** xterm's own screen: the element `scaleToFit` measures a row's cost on. */
  const screenRef = useRef<HTMLElement | null>(null)
  const streamRef = useRef<SessionLogStream | null>(null)
  const [streamStatus, setStreamStatus] = useState<SessionLogStatus>("connecting")
  /**
   * The stream's status as the stream's own callbacks see it. The state above
   * is for the status line; this is for deciding what a change *was*, which a
   * callback created once cannot read off state it closed over.
   */
  const streamStatusRef = useRef<SessionLogStatus>("connecting")
  /**
   * The grid this frame last asked the pane for, and the timer carrying the
   * next such request.
   *
   * The request is not repeated for a size already asked for — and in
   * particular not made again because the pane came back at a different one.
   * Two panels open on the same session each fit it to their own frame, the
   * last one wins, and the loser scales its font to what it was given; a fit
   * that answered the pane's new grid would have the two of them resizing it
   * at each other for as long as both stayed open.
   */
  const requestedSizeRef = useRef<PaneSize | null>(null)
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  /**
   * The session a fit may still be asked for, and whether it has a pane left
   * to ask. A frame takes 200ms to settle and a session can end inside it, or
   * the panel can move to another one — either way the fit waiting in the
   * timer is about something that is no longer on screen.
   *
   * Kept, and cleared, in a *layout* effect. A passive one is too late: React
   * schedules those, and a timer that came due while the change was being
   * rendered runs before they are flushed — it would find a guard that still
   * said the old session was live and ask for its pane. Layout effects run
   * inside the commit, so there is no moment between the change and the
   * cancellation for the timer to land in.
   */
  const fitTargetRef = useRef({ sessionId, live })
  useLayoutEffect(() => {
    fitTargetRef.current = { sessionId, live }
    return () => {
      if (resizeTimerRef.current !== null) clearTimeout(resizeTimerRef.current)
      resizeTimerRef.current = null
    }
  }, [sessionId, live])
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
   * Every pass costs a reflow, since it reads back a layout the pass before it
   * dirtied, so a pass is spent only where it can still learn something: both
   * measurements are taken together and ahead of the one write, and a pass
   * that measures exactly what the last one did stops instead of computing the
   * same answer a third and fourth time.
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
    let measured: string | null = null
    for (let pass = 0; pass < MAX_SCALE_PASSES; pass++) {
      const proposed = fit.proposeDimensions()
      const height = screenRef.current?.clientHeight ?? 0
      if (!proposed || !Number.isFinite(proposed.cols)) return
      // The same numbers as the pass before means the font it wrote changed
      // nothing the frame can be measured by, and the scale below would only
      // grow it again off a measurement that never followed.
      const measurement = `${proposed.cols}x${height}`
      if (measurement === measured) return
      measured = measurement
      const current = terminal.options.fontSize ?? BASE_FONT_SIZE
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

  // The emulator outlives every frame it is scaled against — expanding moves
  // it rather than rebuilding it — so its own listeners reach the current
  // scaling through a ref instead of holding the one it was created with.
  const scaleToFitRef = useRef(scaleToFit)
  useEffect(() => {
    scaleToFitRef.current = scaleToFit
  }, [scaleToFit])

  /**
   * Ask the pane for the grid this frame has room for, once the frame has
   * stopped moving.
   *
   * Everything happens on the far side of the wait, measurement included. A
   * frame mid-drag is a frame being measured over and over for a size that is
   * about to change, and — the reason it *has* to wait — the numbers a fit
   * reads are only true once the emulator has finished laying itself out. A
   * grid that just changed, or a webfont that landed after the terminal
   * opened, both leave a screen whose height belongs to the cells it had a
   * moment ago; measured then, a row costs the wrong number of pixels and the
   * pane is asked for a grid that does not fit the frame.
   *
   * The font is put back to {@link BASE_FONT_SIZE} before measuring, since
   * that is the size the pane is meant to be drawn at: a font `scaleToFit`
   * shrank while the last request was in flight measures the frame in smaller
   * cells, and the grid that came back would be one the base font cannot fit.
   * Everything else the measurement needs is in {@link paneFit} — including
   * why the rows are not the fit addon's to propose.
   *
   * A size this frame has already asked for is not asked for again. That is
   * what keeps two panels on one session from resizing it at each other:
   * neither one's measurement changes because the pane's grid did, so neither
   * answers the other's fit. What does change a measurement is the frame
   * moving, or the emulator settling on cells of a different size — and both
   * of those are worth another request.
   *
   * The request itself is fire-and-forget, like a keystroke: nothing here
   * waits for it, nothing re-renders for it, and a refusal is not worth a
   * toast — the terminal keeps working at the grid it has, scaled to the
   * frame, and the next thing that moves the frame asks again.
   */
  const scheduleFit = useCallback(() => {
    if (resizeTimerRef.current !== null) clearTimeout(resizeTimerRef.current)
    resizeTimerRef.current = setTimeout(() => {
      resizeTimerRef.current = null
      const terminal = terminalRef.current
      const fit = fitRef.current
      const container = containerRef.current
      // A session that ended while its frame was settling has no pane left to
      // ask — the request would be a 409 for a grid nobody would draw at —
      // and a panel that moved to another session would be asking about the
      // wrong pane entirely. Both are dropped, and the font is left to fit
      // whatever is on screen.
      const target = fitTargetRef.current
      if (!target.live || target.sessionId !== sessionId) return
      if (!terminal || !fit || !container) return
      if (terminal.options.fontSize !== BASE_FONT_SIZE) {
        terminal.options.fontSize = BASE_FONT_SIZE
      }
      const size = paneFit({
        cols: fit.proposeDimensions()?.cols,
        rows: terminal.rows,
        screenHeight: screenRef.current?.clientHeight ?? 0,
        heightBudget: screenHeightBudget(expanded ? frameRef.current : null, container),
      })
      if (size && !sameSize(size, requestedSizeRef.current)) {
        requestedSizeRef.current = size
        sendSessionResize(sessionId, size).catch(() => {
          // The pane stayed the size it was, which the stream already agrees
          // with and the font already fits to. Nothing on screen is wrong, so
          // nothing is said about it.
        })
      }
      // The frame is still showing the grid it had, at a font just put back to
      // the base size: whatever of it does not fit is scaled until the pane
      // answers.
      scaleToFitRef.current()
    }, RESIZE_DEBOUNCE_MS)
  }, [expanded, sessionId])

  /**
   * Fit the terminal to the frame it is in: the pane's own grid where there is
   * a pane to ask, the font meanwhile.
   *
   * Both, because a resize is a round trip. The frame goes on showing the grid
   * it has until the pane answers with the new one, and scaling covers exactly
   * that gap — as it covers a pane sized by somebody else, and a session that
   * has ended and cannot be asked at all.
   */
  const refit = useCallback(() => {
    if (live) scheduleFit()
    scaleToFit()
  }, [live, scheduleFit, scaleToFit])

  // Same reason as `scaleToFitRef`, for the emulator's and the stream's own
  // callbacks: they are made once and the fit they reach has to be the
  // current frame's.
  const refitRef = useRef(refit)
  useEffect(() => {
    refitRef.current = refit
  }, [refit])

  // The emulator itself: created once, and kept across daemon-URL changes so a
  // reconnect does not flash an empty box — and across the move between the
  // panel and the expanded dialog, which is why nothing this effect reads may
  // change with the frame.
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
    // Written by `open`, and the same element for as long as the emulator
    // lives: looked up once rather than on every scaling pass.
    screenRef.current = container.querySelector<HTMLElement>(".xterm-screen")

    // The GPU renderer, when this browser has one to give. Fetched on demand
    // rather than imported with the module, so a browser that cannot use it
    // never downloads it — and the DOM renderer is already drawing by the time
    // it lands, which is what makes every way of not getting one a fallback
    // and not a failure.
    let webgl: WebglAddon | null = null
    let disposed = false
    void (async () => {
      if (!supportsWebgl()) return
      try {
        const { WebglAddon } = await import("@xterm/addon-webgl")
        if (disposed) return
        const addon = new WebglAddon()
        terminal.loadAddon(addon)
        webgl = addon
        // The context can be taken back at any time — another tab exhausting
        // the GPU, a driver reset — and xterm draws through the DOM again as
        // soon as the addon is disposed. Which is the whole recovery: a new
        // addon would ask the same driver for the same context.
        addon.onContextLoss(() => {
          webgl = null
          addon.dispose()
        })
      } catch {
        // No WebGL2 context for the addon, or no addon: the renderer xterm
        // opened with is still drawing, and that is the fallback.
      }
    })()

    // Scrolled up into the history, the viewer stops seeing what the agent is
    // doing now; the frame offers a way back rather than leaving them there.
    const scrolled = terminal.onScroll(() => {
      const buffer = terminal.buffer.active
      setFollowing(buffer.viewportY >= buffer.baseY)
    })

    // The grid arrives in the stream and applies whenever the parser reaches
    // it, so the emulator itself — not the message that caused it — is what
    // says a new one is in effect and the terminal has to be fitted to it.
    //
    // Fitted, and not merely scaled: a grid change is also the moment the
    // emulator's cells may settle on a size the last fit did not measure — a
    // webfont that landed after the terminal opened is the usual way — and a
    // fit measured against the wrong cell height asked for a grid that does
    // not fit the frame. Measuring again cannot run away with itself, because
    // a measurement that has not changed is not a request (see
    // {@link scheduleFit}).
    const resized = terminal.onResize(() => refitRef.current())

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
      disposed = true
      container.removeEventListener("click", focusTerminal)
      container.removeEventListener("focusin", focusIn)
      container.removeEventListener("focusout", focusOut)
      focusChangeRef.current(false)
      scrolled.dispose()
      resized.dispose()
      terminalRef.current = null
      fitRef.current = null
      screenRef.current = null
      // Before the terminal, which is what the addon draws for.
      webgl?.dispose()
      terminal.dispose()
    }
  }, [])

  // Resizing the window, the panel or the sheet it sits in re-scales the same
  // grid to what the frame now is.
  useEffect(() => {
    const frame = frameRef.current
    if (!frame) return
    // Also what fits the pane when the terminal is moved between the two
    // frames: the emulator stays, and the room it has to fill is new.
    refit()
    let size: { width: number; height: number } | null = null
    const observer = new ResizeObserver((entries) => {
      // The size comes with the notification, so deciding whether it is one
      // worth a fit costs nothing. Reading it back off the frame would force a
      // layout on every notification instead — including the ones the font
      // this observer just wrote is what caused.
      const measured = entries[entries.length - 1]?.contentRect
      if (!measured) return
      const previous = size
      size = { width: measured.width, height: measured.height }
      // The first notification is the frame as the call above already scaled
      // it to: a baseline, not a resize.
      if (!previous) return
      // In the panel the frame's height follows the terminal's, which follows
      // the font, so reacting to it would be a loop: only a new width is a new
      // fit there. The expanded frame is given its height by the dialog and
      // keeps it whatever the font does, so a window that only got taller is a
      // new fit too.
      const resized =
        measured.width !== previous.width || (expanded && measured.height !== previous.height)
      if (!resized) return
      refit()
    })
    observer.observe(frame)
    return () => observer.disconnect()
  }, [expanded, refit])

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
        // A stream that dropped and came back may be looking at a different
        // pane — a session revived after a daemon restart is a new tmux
        // window, at tmux's own 80×24 — so what this frame last asked for
        // says nothing about what it is showing now, and the fit is made
        // again. A reconnect is safe to react to for the same reason a
        // `resize` event is not: nothing another viewer does causes one.
        if (next === "live" && streamStatusRef.current === "reconnecting") {
          requestedSizeRef.current = null
          refitRef.current()
        }
        streamStatusRef.current = next
        setStreamStatus(next)
        if (why !== undefined) setError(why)
      },
    })
    streamRef.current = stream
    stream.start()
    return () => {
      streamRef.current = null
      stream.stop()
      // Another session, or another daemon, is another pane: whatever this
      // frame asked the last one for says nothing about the next.
      requestedSizeRef.current = null
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

/**
 * Whether this browser can hand xterm a GPU renderer at all.
 *
 * Asked before the addon is fetched, so a browser that cannot run it never
 * downloads it. The probe's context is given straight back: contexts are a
 * scarce per-page resource, and one held open for a yes-or-no answer is one
 * the renderer itself may then not get.
 */
function supportsWebgl(): boolean {
  try {
    const context = document.createElement("canvas").getContext("webgl2")
    if (!context) return false
    context.getExtension("WEBGL_lose_context")?.loseContext()
    return true
  } catch {
    return false
  }
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
