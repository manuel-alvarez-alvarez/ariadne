/**
 * Ariadne's own console for an `acp` session: the counterpart of
 * `session-terminal.tsx` for a session with no pane at all.
 *
 * An `acp` session has nothing a tmux pane would show — no grid, no escape
 * sequences — so it gets a console instead: the transcript
 * (`console-transcript.tsx`), an input line, and permission questions
 * answered inline. `console-stream.ts` is the wire underneath, the same shape
 * `log-stream.ts` gives a pane.
 *
 * The panel-to-dialog lift is the same move `session-terminal.tsx` makes, for
 * the same reason: only the frame changes, so expanding costs no new
 * connection and drops nothing already on screen. There is no escape-key
 * tug-of-war to referee here, though — a plain input line does not want
 * Escape the way a pane's TUI does, so the dialog closes on it like any other.
 */

import { Minimize2Icon } from "lucide-react"
import { useCallback, useState } from "react"
import { createPortal } from "react-dom"

import type { SessionStatus } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"

import { ConsoleView } from "./console-view"

export function AcpConsole({
  sessionId,
  status,
  className,
  screenClassName,
  autoFocus,
}: {
  sessionId: string
  status: SessionStatus
  className?: string
  screenClassName?: string
  /** Hand the input line the keyboard as soon as it is on screen. */
  autoFocus?: boolean
}) {
  const [expanded, setExpanded] = useState(false)
  /**
   * The element the console is rendered into, made once and kept for as long
   * as this component is on screen — the same trick `session-terminal.tsx`
   * uses, for the same reason: moving the DOM node between the panel and the
   * dialog keeps the one stream connection open instead of dropping it and
   * fetching a fresh snapshot for a transcript that has not gone anywhere.
   */
  const [host] = useState(createConsoleHost)

  const anchor = useCallback(
    (node: HTMLDivElement | null) => {
      node?.append(host)
    },
    [host],
  )

  const view = createPortal(
    <ConsoleView
      sessionId={sessionId}
      status={status}
      className={className}
      screenClassName={screenClassName}
      expanded={expanded}
      autoFocus={autoFocus}
      onExpandedChange={setExpanded}
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
      <EmptyState
        emphasis="quiet"
        title="The console is open in the expanded view"
        action={
          <Button variant="outline" size="sm" onClick={() => setExpanded(false)}>
            <Minimize2Icon />
            Back to the panel
          </Button>
        }
      />
      <Dialog open onOpenChange={(open) => !open && setExpanded(false)}>
        <DialogContent
          showCloseButton={false}
          className="h-[calc(100dvh-2rem)] w-[calc(100vw-2rem)] max-w-[calc(100vw-2rem)] grid-rows-[minmax(0,1fr)] sm:max-w-[calc(100vw-2rem)]"
        >
          <DialogTitle className="sr-only">Console of the session</DialogTitle>
          <div ref={anchor} className="contents" />
        </DialogContent>
      </Dialog>
    </>
  )
}

/** See {@link SessionTerminal}'s `createTerminalHost`, which this mirrors. */
function createConsoleHost(): HTMLDivElement {
  const host = document.createElement("div")
  host.style.display = "contents"
  return host
}
