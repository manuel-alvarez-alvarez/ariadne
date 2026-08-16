/**
 * How the diff viewer is set up for *this* reader, remembered across reloads.
 *
 * Word wrap is how someone reads a diff, not a property of the diff itself, so
 * it is local state next to the collapsed lanes rather than something a shared
 * `?tab=diff` link carries. A store that cannot be read or written (a webview
 * with storage disabled) degrades to the default — wrapping on, which is what
 * the viewer always did — rather than breaking the tab.
 */

import { useCallback, useState } from "react"

export const DIFF_WRAP_KEY = "ariadne.tasks.diff-wrap"

/** Only an explicit "off" turns wrapping off; anything else is the default. */
export function parseWrap(raw: string | null): boolean {
  return raw !== "off"
}

export function serializeWrap(wrap: boolean): string {
  return wrap ? "on" : "off"
}

export function useDiffWrap(): { wrap: boolean; setWrap: (wrap: boolean) => void } {
  const [wrap, setState] = useState(load)

  const setWrap = useCallback((next: boolean) => {
    setState(next)
    save(next)
  }, [])

  return { wrap, setWrap }
}

function load(): boolean {
  try {
    return parseWrap(localStorage.getItem(DIFF_WRAP_KEY))
  } catch {
    return true
  }
}

function save(wrap: boolean): void {
  try {
    localStorage.setItem(DIFF_WRAP_KEY, serializeWrap(wrap))
  } catch {
    // A viewer that cannot remember the preference still wraps or does not.
  }
}
