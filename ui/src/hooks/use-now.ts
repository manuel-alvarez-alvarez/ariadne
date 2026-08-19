/**
 * A clock that ticks for every "3m ago" on screen at once.
 *
 * Relative timestamps have to re-render on their own — a session that goes
 * quiet stops producing events, and its "last activity" would otherwise sit at
 * whatever it said when the row last rendered. One shared interval drives all
 * of them rather than one timer per row.
 *
 * It lives here rather than under a feature because every screen has a
 * timestamp on it: the board's cards, the attention strip, the tables and the
 * panels all read the same tick through {@link import("@/components/when").When}.
 */

import { useSyncExternalStore } from "react"

/** Coarse on purpose: nothing here is displayed with second precision. */
const TICK_MS = 15_000

const listeners = new Set<() => void>()
let timer: ReturnType<typeof setInterval> | null = null
let now = Date.now()

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  if (timer === null) {
    timer = setInterval(() => {
      now = Date.now()
      for (const notify of listeners) notify()
    }, TICK_MS)
  }
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0 && timer !== null) {
      clearInterval(timer)
      timer = null
    }
  }
}

function getSnapshot(): number {
  // The interval only runs while something is subscribed, so a screen mounting
  // after a long idle stretch would otherwise read an ancient clock. Two calls
  // within a tick still return the same value, which is what React requires.
  //
  // Stale in either direction: a clock that was stepped backwards — an NTP
  // correction, a machine woken with the wrong time — is as wrong as one left
  // behind, and the interval would take a whole tick to notice.
  if (Math.abs(Date.now() - now) >= TICK_MS) now = Date.now()
  return now
}

/** Current time in ms, re-rendering the caller every {@link TICK_MS}. */
export function useNow(): number {
  return useSyncExternalStore(subscribe, getSnapshot)
}
