/**
 * What a session's console keeps while it is off screen: the draft, the
 * transcript's scroll position, whether it follows the newest block, the view
 * shown, and the fold preferences.
 *
 * Expanding the console into the modal, collapsing it back and switching the
 * panel's tab each unmount one pane and mount another, so the state cannot
 * live in the pane. It lives here, in memory only: a browser restart starts
 * afresh. Each entry is scoped by the daemon's address and the session, so two
 * daemons, or two sessions, never share a draft.
 *
 * Block folds and the transcript position are the interface the transcript
 * redesign builds on: `blockFolds` holds one preference per block id, and
 * `scrollTop` with `following` holds the reader's place. Write them with
 * `updateViewState`; a block with no preference follows the console's own
 * `folded`.
 */

/** A message the daemon refused, kept so it can be sent again or edited. */
export type FailedSend = {
  id: string
  text: string
  /** The question it answered; none for a message to the console. */
  requestId: string | undefined
  message: string
}

export type ConsoleViewState = {
  draft: string
  /** The scroll box's offset from the top when the reader last scrolled. */
  scrollTop: number
  /** The first block in view and where its top sat below the scroll box's top. */
  anchor: { blockId: string; offset: number } | undefined
  /** The oldest event id the console had loaded; a pane mounted again loads back to it. */
  oldest: string | undefined
  /** True while the reader is at the newest block. */
  following: boolean
  /** The whole console's fold: true folds long output and diffs. */
  folded: boolean
  /** A fold preference per block id; a block with none follows `folded`. */
  blockFolds: Record<string, boolean>
  /** The child whose view is shown; none for `main`. */
  shown: string | undefined
  /** The sends the daemon refused, each with its Retry and Edit. */
  failed: FailedSend[]
}

const initial: ConsoleViewState = {
  draft: "",
  scrollTop: 0,
  anchor: undefined,
  oldest: undefined,
  following: true,
  folded: true,
  blockFolds: {},
  shown: undefined,
  failed: [],
}

const states = new Map<string, ConsoleViewState>()

/** The key of one console: the daemon it talks to and the session. */
export function consoleViewKey(baseUrl: string, sessionId: string): string {
  return `${baseUrl}\n${sessionId}`
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
