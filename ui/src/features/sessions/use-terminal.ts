/**
 * The xterm emulator behind a session's panel: built once, kept for as long as
 * the panel is on screen, typed into while the session is live, and kept fitted
 * to whatever frame it is drawn in.
 *
 * The emulator runs at the pane's grid, never at the frame's. A pane's TUI
 * addresses the cursor and erases lines in *that* grid: rendered even a column
 * wider, every line that wraps in the pane but not here shifts the rows below
 * it, and each repaint lands on the wrong one. So the grid is never fitted to
 * the frame here — it changes when the daemon says the pane changed, and only
 * then (see `log-stream.ts`).
 *
 * What the frame does instead is *ask* for the pane it wants. A tmux pane is
 * 80×24 until a client attaches to it and a browser never does, so a live
 * session measures the room it has at {@link BASE_FONT_SIZE} and posts that
 * grid to `POST /v1/sessions/{id}/resize` — the attach it cannot make. Scaling
 * the font is what is left for the cases that cannot ask: a session that is
 * over, whose replay was written at the size its pane had then, and the moment
 * between asking for a grid and being given it. The arithmetic of both is in
 * `pane-fit.ts`; the emulator itself is `terminal-emulator.ts`.
 *
 * Typing goes the other way, to `POST /v1/sessions/{id}/input`, and only for a
 * live session — a finished one has no pane to type into, so its terminal stays
 * display-only rather than swallowing keystrokes. Nothing is echoed locally:
 * what appears on screen is what tmux sent back through the log stream, exactly
 * as it would in a real attach.
 */

import type { FitAddon } from "@xterm/addon-fit"
import type { Terminal } from "@xterm/xterm"
import { useTheme } from "next-themes"
import { type RefObject, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react"
import { toast } from "sonner"

import { describeError } from "@/lib/format"

import type { PaneSize } from "./log-stream"
import {
  BASE_FONT_SIZE,
  EXPANDED_MAX_FONT_SIZE,
  MAX_FONT_SIZE,
  MAX_SCREEN_HEIGHT,
  nextFontSize,
  paneFit,
  sameSize,
} from "./pane-fit"
import { sendSessionInput, sendSessionResize } from "./queries"
import { DARK_THEME, LIGHT_THEME } from "./terminal-chrome"
import { openTerminal } from "./terminal-emulator"

/**
 * How long a frame has to stop moving before the pane is asked to match it.
 *
 * Dragging a panel's edge is a stream of sizes and every one of them would be a
 * `resize-window` and a full repaint of the pane; what the pane is asked for is
 * where the drag came to rest.
 */
const RESIZE_DEBOUNCE_MS = 200

/**
 * How many times the font may be measured and corrected. Each pass is a
 * reflow, and a pass that measures exactly what the last one did stops on its
 * own, so this is only the ceiling for a frame that keeps moving under it.
 */
const MAX_SCALE_PASSES = 4

interface TerminalHandle {
  /** The box the frame lays out; what the room is measured against. */
  frameRef: RefObject<HTMLDivElement | null>
  /** The element the emulator draws into, inside the frame. */
  containerRef: RefObject<HTMLDivElement | null>
  /** The emulator itself, once it is open. */
  terminalRef: RefObject<Terminal | null>
  /** Ask the pane for this frame's grid, and scale the font meanwhile. */
  refit: () => void
  /**
   * Forget the grid this frame last asked for. A stream that dropped and came
   * back may be looking at a different pane — a session revived after a daemon
   * restart is a new tmux window, at tmux's own 80×24 — so what was asked for
   * says nothing about what is on screen now.
   */
  forgetRequestedSize: () => void
  /** Whether the viewport is on the newest output rather than up in the history. */
  following: boolean
}

export function useTerminal({
  sessionId,
  live,
  expanded,
  onFocusChange,
}: {
  sessionId: string
  /** Whether the session can still be typed into and resized. */
  live: boolean
  /** Whether this is the expanded frame, which has its own room and ceiling. */
  expanded: boolean
  onFocusChange: (focused: boolean) => void
}): TerminalHandle {
  const { resolvedTheme } = useTheme()
  const [following, setFollowing] = useState(true)

  // Held in a ref so the emulator's own listeners reach the current callback
  // without the emulator being torn down and rebuilt for it.
  const focusChangeRef = useRef(onFocusChange)
  useEffect(() => {
    focusChangeRef.current = onFocusChange
  }, [onFocusChange])

  const frameRef = useRef<HTMLDivElement | null>(null)
  const containerRef = useRef<HTMLDivElement | null>(null)
  const terminalRef = useRef<Terminal | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const screenRef = useRef<HTMLElement | null>(null)
  /**
   * The grid this frame last asked the pane for, and the timer carrying the
   * next such request.
   *
   * The request is not repeated for a size already asked for — and in
   * particular not made again because the pane came back at a different one.
   * Two panels open on the same session each fit it to their own frame, the
   * last one wins, and the loser scales its font to what it was given; a fit
   * that answered the pane's new grid would have the two of them resizing it at
   * each other for as long as both stayed open.
   */
  const requestedSizeRef = useRef<PaneSize | null>(null)
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  /**
   * The session a fit may still be asked for, and whether it has a pane left to
   * ask. A frame takes 200ms to settle and a session can end inside it, or the
   * panel can move to another one — either way the fit waiting in the timer is
   * about something that is no longer on screen.
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

  /**
   * Fit the pane's grid into the frame by scaling the font — the one dimension
   * that is ours to choose.
   *
   * Every pass costs a reflow, since it reads back a layout the pass before it
   * dirtied, so a pass is spent only where it can still learn something: both
   * measurements are taken together and ahead of the one write, and a pass that
   * measures exactly what the last one did stops instead of computing the same
   * answer a third and fourth time.
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
    const heightBudget = screenHeightBudget(expanded ? frameRef.current : null, container)
    const ceiling = expanded ? EXPANDED_MAX_FONT_SIZE : MAX_FONT_SIZE
    let measured: string | null = null
    for (let pass = 0; pass < MAX_SCALE_PASSES; pass++) {
      const proposed = fit.proposeDimensions()
      const screenHeight = screenRef.current?.clientHeight ?? 0
      if (!proposed || !Number.isFinite(proposed.cols)) return
      // The same numbers as the pass before means the font it wrote changed
      // nothing the frame can be measured by, and the scale below would only
      // grow it again off a measurement that never followed.
      const measurement = `${proposed.cols}x${screenHeight}`
      if (measurement === measured) return
      measured = measurement
      const current = terminal.options.fontSize ?? BASE_FONT_SIZE
      const next = nextFontSize({
        current,
        proposedCols: proposed.cols,
        gridCols: terminal.cols,
        screenHeight,
        heightBudget,
        ceiling,
      })
      if (next === current) return
      terminal.options.fontSize = next
    }
  }, [expanded])

  // The emulator outlives every frame it is scaled against — expanding moves it
  // rather than rebuilding it — so its own listeners reach the current scaling
  // through a ref instead of holding the one they were created with.
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
   * grid that just changed, or a webfont that landed after the terminal opened,
   * both leave a screen whose height belongs to the cells it had a moment ago.
   *
   * The font is put back to {@link BASE_FONT_SIZE} before measuring, since that
   * is the size the pane is meant to be drawn at: a font `scaleToFit` shrank
   * while the last request was in flight measures the frame in smaller cells,
   * and the grid that came back would be one the base font cannot fit.
   *
   * The request itself is fire-and-forget, like a keystroke: nothing here waits
   * for it, nothing re-renders for it, and a refusal is not worth a toast — the
   * terminal keeps working at the grid it has, scaled to the frame, and the
   * next thing that moves the frame asks again.
   */
  const scheduleFit = useCallback(() => {
    if (resizeTimerRef.current !== null) clearTimeout(resizeTimerRef.current)
    resizeTimerRef.current = setTimeout(() => {
      resizeTimerRef.current = null
      const terminal = terminalRef.current
      const fit = fitRef.current
      const container = containerRef.current
      // A session that ended while its frame was settling has no pane left to
      // ask, and a panel that moved to another session would be asking about
      // the wrong pane entirely. Both are dropped, and the font is left to fit
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
  // callbacks: they are made once and the fit they reach has to be the current
  // frame's.
  const refitRef = useRef(refit)
  useEffect(() => {
    refitRef.current = refit
  }, [refit])

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
      // layout on every notification instead — including the ones the font this
      // observer just wrote is what caused.
      const measured = entries[entries.length - 1]?.contentRect
      if (!measured) return
      const previous = size
      size = { width: measured.width, height: measured.height }
      // The first notification is the frame as the call above already scaled it
      // to: a baseline, not a resize.
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

  // The emulator itself: created once, and kept across daemon-URL changes so a
  // reconnect does not flash an empty box — and across the move between the
  // panel and the expanded dialog, which is why nothing this effect reads may
  // change with the frame.
  useEffect(() => {
    const container = containerRef.current
    if (!container) return
    const open = openTerminal(container, {
      onFollowingChange: setFollowing,
      onResize: () => refitRef.current(),
      onFocusChange: (focused) => focusChangeRef.current(focused),
    })
    terminalRef.current = open.terminal
    fitRef.current = open.fit
    screenRef.current = open.screen
    return () => {
      terminalRef.current = null
      fitRef.current = null
      screenRef.current = null
      open.dispose()
    }
  }, [])

  useEffect(() => {
    const terminal = terminalRef.current
    if (!terminal) return
    terminal.options.theme = resolvedTheme === "dark" ? DARK_THEME : LIGHT_THEME
  }, [resolvedTheme])

  // Keystrokes to the pane. `onData` hands over what a real terminal would have
  // written to the pty — `\r` for Return, `\x03` for Ctrl-C, `\x1b[A` for Up —
  // which the daemon forwards to tmux verbatim, so it is sent as-is and never
  // echoed here: the echo arrives with the rest of the pane's output.
  //
  // Re-attached whenever the session's liveness changes, so a session that ends
  // while it is on screen stops accepting input on the spot.
  useEffect(() => {
    const terminal = terminalRef.current
    if (!terminal) return
    terminal.options.disableStdin = !live
    if (!live) return
    const input = terminal.onData((data) => {
      sendSessionInput(sessionId, data).catch((error: unknown) => {
        // One toast per session, not per keystroke: whatever refuses the input
        // refuses all of it, and a burst of typing would otherwise bury the
        // screen in identical toasts.
        toast.error("Could not send that keystroke", {
          id: `session-input-${sessionId}`,
          description: describeError(error),
        })
      })
    })
    return () => input.dispose()
  }, [live, sessionId])

  const forgetRequestedSize = useCallback(() => {
    requestedSizeRef.current = null
  }, [])

  return { frameRef, containerRef, terminalRef, refit, forgetRequestedSize, following }
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
