/**
 * The terminal as it renders in one of its two frames — the panel it belongs
 * to, or the dialog it was expanded into.
 *
 * What is here is the log stream and the status line above the emulator; the
 * emulator itself, and keeping it fitted to whichever frame is showing, is
 * {@link useTerminal}'s.
 */

import {
  ArrowDownToLineIcon,
  KeyboardIcon,
  Maximize2Icon,
  Minimize2Icon,
  RotateCwIcon,
} from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { SessionStatus } from "@/api"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/format"
import { useBaseUrl } from "@/stores/settings"

import { type SessionLogStatus, SessionLogStream, sessionLogStreamUrl } from "./log-stream"
import { isLiveStatus } from "./session-display"
import { ConnectingScreen, StreamStatus } from "./terminal-chrome"
import { writeDelta, writeResize, writeSnapshot } from "./terminal-sink"
import { useTerminal } from "./use-terminal"

export function TerminalView({
  sessionId,
  status,
  className,
  screenClassName,
  expanded,
  autoFocus,
  onExpandedChange,
  onFocusChange,
}: {
  sessionId: string
  status: SessionStatus
  className?: string
  screenClassName?: string
  /** Whether this is the expanded view rather than the panel's. */
  expanded: boolean
  /** Hand the pane the keyboard on mount; see {@link SessionDetailView}. */
  autoFocus?: boolean
  onExpandedChange: (expanded: boolean) => void
  onFocusChange: (focused: boolean) => void
}) {
  const live = isLiveStatus(status)
  const baseUrl = useBaseUrl()
  const { containerRef, following, forgetRequestedSize, frameRef, refit, terminalRef } =
    useTerminal({ sessionId, live, expanded, onFocusChange })
  const streamRef = useRef<SessionLogStream | null>(null)
  const [streamStatus, setStreamStatus] = useState<SessionLogStatus>("connecting")
  /**
   * The stream's status as the stream's own callbacks see it. The state above
   * is for the status line; this is for deciding what a change *was*, which a
   * callback created once cannot read off state it closed over.
   */
  const streamStatusRef = useRef<SessionLogStatus>("connecting")
  const [error, setError] = useState<string | null>(null)
  /** Whether anything has been drawn yet; until then the frame is a placeholder. */
  const [attached, setAttached] = useState(false)

  // The stream's callbacks are made once per session; the fit they reach has to
  // be the current frame's.
  const refitRef = useRef(refit)
  refitRef.current = refit

  // One stream per session and daemon. Every connection opens with a full
  // snapshot, so a reconnect replaces the contents instead of appending to them
  // — splicing a fresh capture onto stale output would invent a history the
  // agent never printed. See `terminal-sink.ts` for why "replaces" is a write
  // and not a `reset()` call, and why the grid is a write too.
  useEffect(() => {
    const stream = new SessionLogStream(sessionLogStreamUrl(baseUrl, sessionId), {
      onResize: (size) => {
        if (terminalRef.current) writeResize(terminalRef.current, size.cols, size.rows)
      },
      onSnapshot: (chunk) => {
        if (terminalRef.current) writeSnapshot(terminalRef.current, chunk)
        setAttached(true)
      },
      onDelta: (chunk) => {
        if (terminalRef.current) writeDelta(terminalRef.current, chunk)
      },
      onEnd: () => {},
      onStatus: (next, why) => {
        // A stream that dropped and came back may be looking at a different
        // pane, so the fit is made again. A reconnect is safe to react to for
        // the same reason a `resize` event is not: nothing another viewer does
        // causes one.
        if (next === "live" && streamStatusRef.current === "reconnecting") {
          forgetRequestedSize()
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
      forgetRequestedSize()
    }
  }, [baseUrl, sessionId, terminalRef, forgetRequestedSize])

  // The emulator is opened by `useTerminal` above, whose effect is registered
  // — and so runs — before this one; a session with no pane left is not typed
  // into and does not take the keyboard away from the rest of the panel.
  useEffect(() => {
    if (!autoFocus || !live) return
    terminalRef.current?.focus()
  }, [autoFocus, live, terminalRef])

  const expandLabel = expanded ? "Collapse the terminal back into the panel" : "Expand the terminal"

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
            aria-label={expandLabel}
            title={expandLabel}
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
          // All that is left of the dialog, and the height the font is scaled
          // against there.
          expanded && "min-h-0 flex-1",
          screenClassName,
        )}
      >
        {/*
          Full width even when the grid is narrower, because it is the width the
          font is measured against — and it is the frame's, not the terminal's
          own. Sideways scrolling is the last resort for a pane too wide to
          shrink into (see MIN_FONT_SIZE).
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
