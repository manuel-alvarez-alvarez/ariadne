/**
 * How much of a thread this reader has not read yet.
 *
 * A panel opens on the description, and a thread it is not showing can gain
 * three messages without anything on screen saying so — the tab trigger looks
 * exactly as it did. So each thread remembers the last message the reader
 * actually had in front of them, and everything the daemon has added past it is
 * what the trigger counts.
 *
 * The mark is one message id per thread in `localStorage`, not a count: ids are
 * ULIDs, so "newer than what I saw" is a comparison rather than a search, and it
 * survives a message the thread no longer carries. It is local storage and not
 * session, unlike a draft ({@link readDraft}): having read a thread is true of
 * this reader tomorrow too.
 *
 * Nothing counts as unread until the thread has been opened once. A goal picked
 * off the board would otherwise announce its whole history as new, which says
 * nothing about what changed — so the first read of a thread records where it
 * is and starts counting from there.
 *
 * *Opening the thread* is the only thing that records it. {@link ThreadView} is
 * what marks it, because that is the component that is the thread being on
 * screen; the count below only ever reads. A panel opens on its description tab
 * and may be closed again without the thread ever being shown, so a hook that
 * recorded a mark because the panel rendered would be claiming the reader saw
 * messages the app never drew for them.
 *
 * The mark is read through `useSyncExternalStore` because two components watch
 * it at once: the thread marks it while the reader is at the bottom of it, and
 * the tab trigger a level up has to stop showing a count on the same gesture.
 */

import { useSyncExternalStore } from "react"

import type { MessageDto } from "@/api"
import type { ThreadKey } from "@/components/thread-drafts"
import { Badge } from "@/components/ui/badge"

const PREFIX = "ariadne.thread-seen."

/** Where the mark goes when `localStorage` refuses to hold it. */
const fallback = new Map<ThreadKey, string>()
const listeners = new Set<() => void>()

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** The last message this reader has seen in the thread, or `null` for a thread never opened. */
function readSeen(key: ThreadKey): string | null {
  try {
    return localStorage.getItem(PREFIX + key)
  } catch {
    return fallback.get(key) ?? null
  }
}

/**
 * Record that the reader has seen everything up to and including this message.
 *
 * Never moves the mark backwards: the thread marks the newest message it is
 * showing, and a stale render must not un-read what a newer one already did.
 */
export function markThreadSeen(key: ThreadKey, messageId: string): void {
  const seen = readSeen(key)
  if (seen !== null && seen >= messageId) return
  try {
    localStorage.setItem(PREFIX + key, messageId)
  } catch {
    fallback.set(key, messageId)
  }
  for (const listener of listeners) listener()
}

/**
 * How many messages of this thread the reader has not seen — nothing at all
 * until they have opened it once, since there is no point to count from.
 *
 * A read and nothing else: what the reader has seen is theirs to change by
 * reading, not by opening the panel a thread hangs off.
 */
export function useUnreadCount(key: ThreadKey, messages: MessageDto[] | undefined): number {
  const seen = useSyncExternalStore(
    subscribe,
    () => readSeen(key),
    () => null,
  )

  if (seen === null || !messages) return 0
  return messages.reduce((count, message) => (message.id > seen ? count + 1 : count), 0)
}

/**
 * The count, on the tab that leads to the thread.
 *
 * Sized down from the badge's own line height: it rides inside a tab trigger,
 * where it is a mark on a label rather than a thing of its own.
 */
export function UnreadBadge({ count }: { count: number }) {
  if (count === 0) return null
  return (
    <Badge
      className="h-4 px-1.5 text-[10px] tabular-nums"
      aria-label={`${count} unread ${count === 1 ? "message" : "messages"}`}
    >
      {count > 99 ? "99+" : count}
    </Badge>
  )
}
