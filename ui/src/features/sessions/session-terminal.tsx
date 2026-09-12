/**
 * A session's console, as the CLI draws it: the same bytes, in a terminal
 * emulator.
 *
 * The daemon runs the console and serves it over a WebSocket as terminal
 * bytes (008, `terminal-socket.ts`); this pane is an xterm.js terminal on
 * that socket. It says its size first and on every refit, writes every
 * binary frame into the terminal, and sends every key press as a `key`
 * message (`terminal-keys.ts`) and every paste as a `paste`. xterm.js's own
 * key handling is bypassed: the console reads keys, not the bytes a terminal
 * would encode them into, so what a key means is decided on the daemon's
 * side, once, for the CLI and for this pane alike.
 *
 * The pane fills the box it is given and refits when that box changes, which
 * the daemon answers by redrawing at the new size. Its colours and font are
 * the app's own (`terminal-theme.ts`), re-read when the theme switches.
 *
 * A drop is retried on the event stream's backoff, and the pane says so
 * meanwhile; a close the daemon meant — the session ended, Ctrl-C twice,
 * Ctrl-D — ends this console, and a button opens another. Every open is a
 * fresh console redrawn whole, so the terminal is reset first rather than
 * left holding a second copy of the transcript.
 */

import "@xterm/xterm/css/xterm.css"

import { FitAddon } from "@xterm/addon-fit"
import { Terminal } from "@xterm/xterm"
import { Loader2Icon, PlugZapIcon } from "lucide-react"
import { useTheme } from "next-themes"
import { useEffect, useRef, useState } from "react"

import type { SessionStatus } from "@/api"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/format"
import { useBaseUrl } from "@/stores/settings"

import { isLiveStatus } from "./session-display"
import { terminalKeyMessage } from "./terminal-keys"
import { TerminalSocket, type TerminalSocketStatus, terminalUrl } from "./terminal-socket"
import { TERMINAL_FONT_SIZE, terminalFontFamily, terminalTheme } from "./terminal-theme"

/** Lines the emulator keeps above the screen. The transcript lives there. */
const SCROLLBACK_LINES = 10_000

export function SessionTerminal({
  sessionId,
  status,
  autoFocus,
  className,
}: {
  sessionId: string
  status: SessionStatus
  /** Hand the terminal the keyboard as soon as it is on screen. */
  autoFocus?: boolean
  className?: string
}) {
  const baseUrl = useBaseUrl()
  const { resolvedTheme } = useTheme()
  const dark = resolvedTheme === "dark"
  const host = useRef<HTMLElement>(null)
  const terminal = useRef<Terminal | null>(null)
  const socket = useRef<TerminalSocket | null>(null)
  const [socketStatus, setSocketStatus] = useState<TerminalSocketStatus>("connecting")
  const [error, setError] = useState<string | null>(null)
  /** The status the daemon last reported on the socket; the cache's until then. */
  const [reported, setReported] = useState<SessionStatus | null>(null)

  // One terminal and one socket per session and daemon, for as long as the
  // pane is on screen. The theme and the focus request are read as the pane
  // opens: the theme has an effect of its own below, and the focus request is
  // read once, on arrival.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the theme is applied by the effect below, and the focus request is read once on mount
  useEffect(() => {
    const el = host.current
    if (!el) return
    let disposed = false
    let teardown: (() => void) | null = null

    // Measured once, on open: a grid measured against the fallback face would
    // be laid out for the wrong cell width once the app's own face arrives.
    void whenMonoFontLoaded().then(() => {
      if (disposed) return
      setReported(null)

      const term = new Terminal({
        fontFamily: terminalFontFamily(),
        fontSize: TERMINAL_FONT_SIZE,
        theme: terminalTheme(dark),
        cursorBlink: true,
        scrollback: SCROLLBACK_LINES,
      })
      const fit = new FitAddon()
      term.loadAddon(fit)
      term.open(el)
      fit.fit()

      const link = new TerminalSocket(terminalUrl(baseUrl, sessionId), {
        onOpen: () => {
          // A fresh console draws the transcript whole: what the last one
          // drew would otherwise sit above it, twice over.
          term.reset()
          fit.fit()
          link.send({ type: "resize", cols: term.cols, rows: term.rows })
        },
        onBytes: (bytes) => term.write(bytes),
        onMessage: (message) => setReported(message.status),
        onStatus: (next, why) => {
          setSocketStatus(next)
          setError(why ?? null)
        },
      })

      term.onResize(({ cols, rows }) => {
        link.send({ type: "resize", cols, rows })
      })
      term.attachCustomKeyEventHandler((event) => {
        if (event.type !== "keydown") return false
        const message = terminalKeyMessage(event)
        if (message === null) return false
        event.preventDefault()
        link.send(message)
        return false
      })
      const onPaste = (event: ClipboardEvent) => {
        const text = event.clipboardData?.getData("text/plain") ?? ""
        // Claimed whether or not there was text: xterm.js would otherwise
        // feed the clipboard through its own input path, which nobody reads.
        event.preventDefault()
        event.stopPropagation()
        if (text.length > 0) link.send({ type: "paste", text })
      }
      el.addEventListener("paste", onPaste, true)
      const observer = new ResizeObserver(() => fit.fit())
      observer.observe(el)

      terminal.current = term
      socket.current = link
      link.start()
      if (autoFocus) term.focus()

      teardown = () => {
        observer.disconnect()
        el.removeEventListener("paste", onPaste, true)
        link.stop()
        term.dispose()
        terminal.current = null
        socket.current = null
      }
    })

    return () => {
      disposed = true
      teardown?.()
    }
  }, [baseUrl, sessionId])

  useEffect(() => {
    const term = terminal.current
    if (term) term.options.theme = terminalTheme(dark)
  }, [dark])

  function reopen() {
    setSocketStatus("connecting")
    setError(null)
    socket.current?.start()
  }

  return (
    <div
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-lg border bg-background font-mono text-xs text-foreground shadow-xs",
        className,
      )}
    >
      <div className="flex items-center justify-between gap-2 border-b px-3 py-1.5 text-muted-foreground">
        <span role="status" className="min-w-0">
          <TerminalStatusLine
            status={socketStatus}
            error={error}
            live={isLiveStatus(reported ?? status)}
          />
        </span>
        {socketStatus === "closed" ? (
          <Button size="xs" variant="outline" onClick={reopen} className="font-mono">
            Reopen
          </Button>
        ) : null}
      </div>
      {/* The padding sits on a box of its own: the fit addon sizes the grid
          from the host's computed width and height, which under border-box
          sizing count the host's padding as room for cells. */}
      <div className="min-h-0 flex-1 p-2">
        <section ref={host} className="h-full" aria-label="Console terminal" />
      </div>
    </div>
  )
}

/**
 * The app's mono face, loaded before the terminal measures a cell on it.
 * Resolves at once where the browser keeps no font set — the test runner —
 * or where the face cannot be asked for, and the fallback is measured.
 */
function whenMonoFontLoaded(): Promise<unknown> {
  const fonts = typeof document === "undefined" ? undefined : document.fonts
  if (fonts === undefined) return Promise.resolve()
  return fonts.load(`${TERMINAL_FONT_SIZE}px ${terminalFontFamily()}`).catch(() => undefined)
}

function TerminalStatusLine({
  status,
  error,
  live,
}: {
  status: TerminalSocketStatus
  error: string | null
  live: boolean
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
  if (status === "closed") {
    return <span>{live ? "The console closed." : "This session has ended."}</span>
  }
  return (
    <span className="flex items-center gap-1.5">
      <span className="size-1.5 shrink-0 animate-pulse rounded-full bg-status-done" aria-hidden />
      Live
    </span>
  )
}
