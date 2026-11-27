/**
 * What a session's console keeps while it is off screen: the draft, the
 * transcript's scroll position, whether it follows the newest block, the view
 * shown, the fold preferences and the opened activity runs.
 *
 * Expanding the console into the modal, collapsing it back and switching the
 * panel's tab each unmount one pane and mount another, so the state cannot
 * live in the pane. It lives here, in memory only: a browser restart starts
 * afresh. Each entry is scoped by the host and the session, and `local`'s also by
 * the daemon's address, so two daemons, or two sessions, never share a draft.
 *
 * `blockFolds` holds one fold preference per turn or plan id, and
 * `scrollTop` with `following` holds the reader's place. Write them with
 * `updateViewState`; a fold with no preference starts closed.
 */

import type { Host } from "@/api"

/** A message the daemon refused, kept so it can be sent again or edited. */
export type FailedSend = {
  id: string
  text: string
  message: string
}

type ConsoleViewState = {
  draft: string
  /** The scroll box's offset from the top when the reader last scrolled. */
  scrollTop: number
  /** The oldest turn id the console had loaded; a pane mounted again loads back to it. */
  oldest: string | undefined
  /** The count of steps before the oldest loaded page of each turn. */
  turnEarlier?: Record<string, number>
  /** True while the reader is at the newest output. */
  following: boolean
  /** A fold preference per turn or plan id; a fold with none starts closed. */
  blockFolds: Record<string, boolean>
  /** The sends the daemon refused, each with its Retry and Edit. */
  failed: FailedSend[]
}

const initial: ConsoleViewState = {
  draft: "",
  scrollTop: 0,
  oldest: undefined,
  following: true,
  blockFolds: {},
  failed: [],
}

const states = new Map<string, ConsoleViewState>()

/**
 * The key of one console: the host and the session. Only `local` adds its
 * daemon address, since the settings can point it at another daemon; a named
 * host keeps its state when its address changes, as an ssh forward picks a new
 * port on each run.
 */
export function consoleViewKey(host: Host, baseUrl: string, sessionId: string): string {
  return `${host}\n${host === "local" ? baseUrl : ""}\n${sessionId}`
}

export function readViewState(key: string): ConsoleViewState {
  return states.get(key) ?? initial
}

const listeners = new Map<string, Set<() => void>>()

/** Merge `patch` into the console's state, and tell the panes showing it. */
export function updateViewState(key: string, patch: Partial<ConsoleViewState>): void {
  states.set(key, { ...readViewState(key), ...patch })
  for (const listener of listeners.get(key) ?? []) listener()
}

/** Call `listener` after every change to the console's state; returns the way to stop. */
export function watchViewState(key: string, listener: () => void): () => void {
  const set = listeners.get(key) ?? new Set()
  set.add(listener)
  listeners.set(key, set)
  return () => {
    set.delete(listener)
    if (set.size === 0) listeners.delete(key)
  }
}
