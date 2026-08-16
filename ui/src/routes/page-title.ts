/**
 * What the shell's header calls the screen it is framing.
 *
 * The title is declared on the route rather than passed down from the shell or
 * lifted out of the page, so a feature that adds a screen names it in the same
 * file it mounts it in — `handle: pageTitle("Sessions")` — and the header needs
 * to know nothing about the route table.
 */

import { useMatches } from "react-router-dom"

export interface PageHandle {
  /** What the header calls this screen; matches its sidebar entry. */
  title: string
}

/** The `handle` for a route that names itself in the header. */
export function pageTitle(title: string): PageHandle {
  return { title }
}

/**
 * The title of the deepest matched route that declares one, or `null` for the
 * screens that do not (the redirects, the not-found page).
 */
export function usePageTitle(): string | null {
  const matches = useMatches()
  for (let i = matches.length - 1; i >= 0; i -= 1) {
    const handle = matches[i]?.handle as Partial<PageHandle> | undefined
    if (handle?.title) return handle.title
  }
  return null
}
