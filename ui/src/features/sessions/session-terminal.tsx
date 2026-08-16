/**
 * The agent's tmux pane, live, in xterm.js.
 *
 * The daemon sends raw terminal bytes — escape sequences, cursor addressing,
 * colours and all — so the output is written into a real terminal emulator
 * rather than rendered as text: anything less would show the control codes of
 * a full-screen TUI instead of what the agent is actually drawing.
 *
 * Typing goes the other way, to `POST /v1/sessions/{id}/input`, and only for a
 * live session — a finished one has no pane to type into, so its terminal
 * stays display-only rather than swallowing keystrokes. Nothing is echoed
 * locally: what appears on screen is what tmux sent back through the log
 * stream, exactly as it would in a real attach.
 */

import { FitAddon } from "@xterm/addon-fit"
import { type ITheme, Terminal } from "@xterm/xterm"
import "@xterm/xterm/css/xterm.css"
import { KeyboardIcon, PlugZapIcon, RotateCwIcon, SquareIcon } from "lucide-react"
import { useTheme } from "next-themes"
import { useEffect, useRef, useState } from "react"
import { toast } from "sonner"

import type { SessionStatus } from "@/api"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { useBaseUrl } from "@/stores/settings"

import { type SessionLogStatus, SessionLogStream, sessionLogStreamUrl } from "./log-stream"
import { sendSessionInput } from "./queries"
import { reason } from "./session-actions"
import { isLiveStatus } from "./session-display"
import { writeDelta, writeSnapshot } from "./terminal-sink"

/** Lines kept above the viewport. A busy agent fills a pane fast. */
const SCROLLBACK = 5_000

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
  /**
   * Classes for the box the emulator draws in — a height above all, which a
   * panel has less of than a page. Merged over the default, so passing one
   * wins (see `cn`).
   */
  screenClassName?: string
}) {
  const live = isLiveStatus(status)
  const baseUrl = useBaseUrl()
  const { resolvedTheme } = useTheme()
  const containerRef = useRef<HTMLDivElement | null>(null)
  const terminalRef = useRef<Terminal | null>(null)
  const streamRef = useRef<SessionLogStream | null>(null)
  const [streamStatus, setStreamStatus] = useState<SessionLogStatus>("connecting")
  const [error, setError] = useState<string | null>(null)

  // The emulator itself: created once, and kept across daemon-URL changes so a
  // reconnect does not flash an empty box.
  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    const terminal = new Terminal({
      // tmux's captured pane ends its lines with a bare newline; without this
      // every line would start where the previous one stopped.
      convertEol: true,
      // Turned on by the input effect below once the session is known to be
      // live; a terminal that cannot reach a pane must not pretend to.
      disableStdin: true,
      cursorBlink: false,
      cursorInactiveStyle: "none",
      scrollback: SCROLLBACK,
      fontFamily: "'Geist Mono Variable', ui-monospace, monospace",
      fontSize: 12,
      lineHeight: 1.2,
      allowTransparency: false,
      // Read off the class `next-themes` puts on <html> rather than from the
      // hook: the hook resolves a tick after mount, and the terminal would
      // flash its own default palette in between.
      theme: currentTerminalTheme(),
    })
    const fit = new FitAddon()
    terminal.loadAddon(fit)
    terminal.open(container)
    terminalRef.current = terminal

    // The pane is not resized by us — this only decides how much of it fits on
    // screen — so a failed measurement (a hidden or zero-sized container) is
    // nothing to report, just nothing to do.
    const refit = () => {
      try {
        fit.fit()
      } catch {
        // container has no usable size yet
      }
    }
    refit()
    const observer = new ResizeObserver(refit)
    observer.observe(container)
    window.addEventListener("resize", refit)

    // xterm focuses itself when its own screen is clicked; this covers the
    // padding around it, so the whole box is somewhere to start typing.
    // Keyboard users need no equivalent — xterm's textarea is in the tab
    // order — which is why it is a DOM listener and not an `onClick` prop.
    const focusTerminal = () => terminal.focus()
    container.addEventListener("click", focusTerminal)

    return () => {
      container.removeEventListener("click", focusTerminal)
      window.removeEventListener("resize", refit)
      observer.disconnect()
      terminalRef.current = null
      terminal.dispose()
    }
  }, [])

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
          description: reason(error),
        })
      })
    })
    return () => input.dispose()
  }, [live, sessionId])

  // One stream per session and daemon. Every connection opens with a full
  // snapshot, so a reconnect replaces the contents instead of appending to
  // them — splicing a fresh capture onto stale output would invent a history
  // the agent never printed. See `terminal-sink.ts` for why "replaces" is a
  // write and not a `reset()` call.
  useEffect(() => {
    const stream = new SessionLogStream(sessionLogStreamUrl(baseUrl, sessionId), {
      onSnapshot: (chunk) => {
        const terminal = terminalRef.current
        if (terminal) writeSnapshot(terminal, chunk)
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
    <div className={cn("flex min-h-0 flex-col gap-2", className)}>
      <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
        <StreamStatus status={streamStatus} error={error} />
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
      </div>
      <div
        ref={containerRef}
        // xterm measures its parent, so the box has to have a size of its own.
        className={cn(
          "h-[28rem] min-h-0 overflow-hidden rounded-lg border bg-card p-2",
          screenClassName,
        )}
      />
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
    return <span>Connecting to the session's output…</span>
  }
  return (
    <span className="flex items-center gap-1.5">
      <span className="size-1.5 shrink-0 animate-pulse rounded-full bg-emerald-500" aria-hidden />
      Live
    </span>
  )
}
