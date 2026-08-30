/**
 * What every test in the app is given before its module is imported.
 *
 * Two of these have to happen *first*, which is why they are here and not in a
 * helper the test imports: `openapi-fetch` takes its `fetch` when the client is
 * built, which is when `@/api` is first imported, so a stub installed after
 * that is one the daemon client never sees — the test would go looking for a
 * real daemon. The jsdom shims are here for the same reason in reverse: they
 * are what a browser has and jsdom does not, and a component that measures
 * itself on mount would throw before any test body ran.
 *
 * Nothing here decides anything: the fetch stub answers nothing until a test
 * says what the daemon should say, and both shims are no-ops a test is free to
 * replace with `vi.stubGlobal`.
 *
 * The teardown below is what every test file was writing for itself, and what
 * one of them forgetting leaves the *next* file to trip over: a rendered tree
 * still in the document, a daemon still answering the last test's questions, a
 * global still stubbed.
 */

import { cleanup } from "@testing-library/react"
import { afterEach, beforeEach, vi } from "vitest"

/** The daemon, as every test sees it. Configured per test, reset between them. */
export const daemonFetch = vi.fn()
globalThis.fetch = daemonFetch as unknown as typeof fetch

beforeEach(() => {
  // Until a test says what the daemon answers, it answers nothing at all: a
  // request that never settles, which is a screen that stays loading. That is
  // what the tests of a screen's chrome want, and it is a far better default
  // than a client throwing on an undefined response.
  daemonFetch.mockImplementation(() => new Promise(() => {}))
})

// jsdom lays nothing out, so it implements neither of these — and it gives
// neither web storage: `localStorage`, which `zustand/middleware` takes hold of
// the moment `@/stores/settings` is imported, and `sessionStorage`. A test
// that cares what is in one of them clears it itself; both survive between
// the tests of a file, exactly as they survive between screens in the app.
if (typeof window !== "undefined") {
  globalThis.localStorage = webStorage()
  globalThis.sessionStorage = webStorage()

  // Assigned rather than stubbed: a test that calls `vi.unstubAllGlobals` in
  // its teardown would otherwise take this with it and leave the next one
  // without one at all.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  Element.prototype.scrollIntoView = vi.fn()
}

afterEach(() => {
  if (typeof window !== "undefined") cleanup()
  daemonFetch.mockReset()
  vi.unstubAllGlobals()
})

/** As much of the `Storage` interface as anything in the app asks for. */
function webStorage(): Storage {
  const entries = new Map<string, string>()
  return {
    get length() {
      return entries.size
    },
    key: (index: number) => [...entries.keys()][index] ?? null,
    getItem: (key: string) => entries.get(key) ?? null,
    setItem: (key: string, value: string) => void entries.set(key, value),
    removeItem: (key: string) => void entries.delete(key),
    clear: () => entries.clear(),
  } as Storage
}
