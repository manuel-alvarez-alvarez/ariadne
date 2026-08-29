/**
 * A conversation as both threads in the app draw one: the messages oldest
 * first, and the compose box under them.
 *
 * The goal thread and the task thread were the same list twice — the same
 * card, the same link to the session that posted it, the same box below — and
 * everything this adds to it has to be true of both, so it is one component
 * they each hand their own messages and mutation to. What they keep for
 * themselves is what differs: the loading and error states, and the surface
 * the thread is drawn on.
 *
 * **It opens at the end.** A thread is read from its newest message, and one
 * that opens at the oldest asks the reader to scroll past everything they have
 * already seen to find out what changed. So the view is put on the end when it
 * mounts, and it *stays* there while the reader is at the end: a message that
 * arrives over the stream scrolls into place under the one before it, the way
 * a conversation anywhere else does.
 *
 * **It stops following the moment the reader scrolls up.** Somebody reading
 * back through the thread is not asking to be dragged to the bottom every time
 * an agent says something, so what arrives while they are up there is counted
 * instead, and offered as a pill that takes them to it. Reaching the bottom
 * again, by the pill or by scrolling, resumes the following.
 *
 * The thread does not scroll itself: the panel around it is the one scroll
 * container (see `@/lib/scroll`), which is what makes the compose box stick to
 * the bottom of the *panel* and a thread on a page behave the same as one in a
 * sheet.
 */

import type { UseMutationResult } from "@tanstack/react-query"
import { ArrowDownIcon } from "lucide-react"
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react"

import type { CreateMessageRequest, MessageDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { MessageCard } from "@/components/message-card"
import { type Addressee, MessageComposer } from "@/components/message-composer"
import type { ThreadKey } from "@/components/thread-drafts"
import { markThreadSeen } from "@/components/thread-unread"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/format"
import { isAtBottom, scrollParent, scrollToBottom } from "@/lib/scroll"

/**
 * What the compose box has to cover as it scrolls over the thread, and what
 * the last message fades into above it. Both have to be the surface the thread
 * is actually drawn on, or the box is a differently coloured strip across it.
 */
const SURFACES = {
  background: { solid: "bg-background", fade: "from-background" },
  card: { solid: "bg-card", fade: "from-card" },
}

export function ThreadView({
  threadKey,
  messages,
  post,
  label,
  placeholder,
  addressees,
  autoFocus,
  presetTo,
  emptyTitle,
  closedHint,
  source,
  surface = "background",
}: {
  /** Which thread this is, for its draft and its unread mark. */
  threadKey: ThreadKey
  /** The thread, oldest first; `undefined` while it is still being loaded. */
  messages: MessageDto[] | undefined
  post: UseMutationResult<MessageDto, Error, CreateMessageRequest>
  label: string
  placeholder: string
  addressees?: readonly Addressee[]
  /**
   * The thread was opened *to answer* somebody — from the attention list — so
   * the box takes the keyboard and starts addressed to whoever asked. Both are
   * the compose box's; they pass through here because the box is its.
   */
  autoFocus?: boolean
  presetTo?: string | null
  emptyTitle: string
  /** Why the box is closed, when nothing is left working this thread. */
  closedHint?: string
  /** Where a message came from, drawn under it — the session that posted it. */
  source?: (message: MessageDto) => ReactNode
  surface?: keyof typeof SURFACES
}) {
  const end = useRef<HTMLDivElement>(null)
  /** The panel this thread scrolls in, once there is a node to find it from. */
  const container = useRef<Element | null>(null)
  /** Whether the reader is on the newest message; read from a scroll handler. */
  const following = useRef(true)
  /** The newest message the reader has been shown; what "new" is counted from. */
  const shown = useRef<string | null>(null)
  const [unseen, setUnseen] = useState(0)

  const newest = messages?.at(-1)?.id ?? null
  const newestRef = useRef(newest)
  newestRef.current = newest

  // The panel can move to another thread without unmounting this view — a
  // stacked task panel pointed at another task does exactly that — and what it
  // lands on is a thread being *opened*, not one that gained messages. Left
  // alone, a reader who had scrolled up in the last one would be shown a count
  // of everything in this one newer than a message from another conversation.
  const thread = useRef(threadKey)
  if (thread.current !== threadKey) {
    thread.current = threadKey
    shown.current = null
    following.current = true
    setUnseen(0)
  }

  /** The reader is at the end: nothing is new, and the thread is read up to here. */
  const catchUp = useCallback(() => {
    shown.current = newestRef.current
    setUnseen(0)
    if (newestRef.current) markThreadSeen(threadKey, newestRef.current)
  }, [threadKey])

  // Whether the reader is at the end is theirs to change at any time, and only
  // the scrollport knows: the thread has no scroll of its own.
  useEffect(() => {
    const panel = scrollParent(end.current)
    container.current = panel
    if (!panel) return
    const onScroll = () => {
      following.current = isAtBottom(panel)
      if (following.current) catchUp()
    }
    panel.addEventListener("scroll", onScroll, { passive: true })
    return () => panel.removeEventListener("scroll", onScroll)
  }, [catchUp])

  // Opening the thread, and every message that lands in it afterwards.
  useEffect(() => {
    if (newest === null || shown.current === newest) return
    const from = shown.current
    if (from === null || following.current) {
      catchUp()
      // Opening lands on the newest message rather than travelling to it: the
      // thread was never anywhere else as far as the reader is concerned.
      const panel = container.current ?? scrollParent(end.current)
      if (panel) scrollToBottom(panel, from === null ? "auto" : "smooth")
      return
    }
    // Read further up the thread: what arrived is counted, not jumped to.
    setUnseen(messages?.reduce((count, m) => (m.id > from ? count + 1 : count), 0) ?? 0)
  }, [newest, messages, catchUp])

  const jumpToEnd = useCallback(() => {
    following.current = true
    catchUp()
    const panel = container.current ?? scrollParent(end.current)
    if (panel) scrollToBottom(panel, "smooth")
  }, [catchUp])

  return (
    <div className="flex flex-col gap-3">
      {messages ? (
        messages.length === 0 ? (
          <EmptyState emphasis="quiet" title={emptyTitle} />
        ) : (
          <ol className="flex flex-col gap-3">
            {messages.map((message) => (
              <li key={message.id}>
                <MessageCard message={message} source={source?.(message)} />
              </li>
            ))}
          </ol>
        )
      ) : null}

      {/* The end of the thread, which is what the panel is scrolled to. */}
      <div ref={end} aria-hidden />

      {/* The box and the pill travel together at the bottom of the panel: the
          pill belongs directly over the box whatever the panel is scrolled to,
          and one sticky footer is what keeps it there without either of them
          knowing how tall the other is. The strip above them is the fade the
          last message scrolls into, so the box crosses the thread with an edge
          that dissolves instead of cutting a message in half. */}
      <div className={cn("sticky bottom-0 z-10 flex flex-col", SURFACES[surface].solid)}>
        <div
          aria-hidden
          className={cn(
            "-mt-3 pointer-events-none h-4 bg-gradient-to-t to-transparent",
            SURFACES[surface].fade,
          )}
        />
        {unseen > 0 ? (
          <div className="flex justify-center pb-2">
            <Button variant="secondary" size="xs" className="shadow-sm" onClick={jumpToEnd}>
              {unseen === 1 ? "1 new message" : `${unseen} new messages`}
              <ArrowDownIcon />
            </Button>
          </div>
        ) : null}
        <MessageComposer
          post={post}
          draftKey={threadKey}
          label={label}
          placeholder={placeholder}
          addressees={addressees}
          autoFocus={autoFocus}
          presetTo={presetTo}
          closedHint={closedHint}
          // Sending is taking part in the thread, so it goes to what was sent
          // even from halfway up the history.
          onSent={jumpToEnd}
        />
      </div>
    </div>
  )
}
