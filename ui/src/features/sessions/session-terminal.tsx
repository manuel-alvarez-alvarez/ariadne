/**
 * The agent's tmux pane, live, in xterm.js.
 *
 * The daemon sends raw terminal bytes — escape sequences, cursor addressing,
 * colours and all — so the output is written into a real terminal emulator
 * rather than rendered as text: anything less would show the control codes of a
 * full-screen TUI instead of what the agent is actually drawing. How that
 * emulator is built, typed into and kept fitted to its frame is
 * `use-terminal.ts`; what it renders inside one is `terminal-view.tsx`.
 *
 * This file is the two frames it can be in. A panel is a small window onto a
 * pane, so the terminal can be lifted into a near-fullscreen dialog, the way
 * `task-diff.tsx` lifts the diff. Only the frame changes: the pane is asked for
 * the dialog's room on the way in and for the panel's on the way out, and a
 * replay that can only be scaled gets a bigger font. The emulator itself makes
 * that trip — it is rendered into an element of its own that is *moved* between
 * the two frames, so expanding costs a re-scale and nothing more: the same
 * emulator, the same open stream, and no snapshot fetched again for output
 * already on screen.
 */

import { Minimize2Icon } from "lucide-react"
import { useCallback, useRef, useState } from "react"
import { createPortal } from "react-dom"

import type { SessionStatus } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"

import { TerminalView } from "./terminal-view"

export function SessionTerminal({
  sessionId,
  status,
  className,
  screenClassName,
}: {
  sessionId: string
  /** Whether the session can still be typed into; see `isLiveStatus`. */
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
      // all. Refs run once the new frame is in the document, so the emulator is
      // never re-parented into a node that is not.
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
          // its own, and Base UI already holds a nested Escape back from it, so
          // nothing else closes on the same press either.
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
