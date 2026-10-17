/**
 * The slot `PageHeader` (`page-header.tsx`) reaches through to put itself in
 * the shell's header, and `AppShell` (`app-shell.tsx`) provides.
 *
 * Split from `app-shell.tsx` on purpose: `PageHeader` is mounted by every
 * feature screen, so a screen's test — and every test of anything that
 * renders one — paid to import the whole shell (the sidebar, the command
 * palette, every dialog it mounts) just to read this context's type.
 */

import { createContext, type ReactNode } from "react"

/**
 * What a mounted `PageHeader` reaches through to put itself in the header:
 * `actionsSlot` is the DOM node its `actions` portal into — the header's end,
 * before search, theme and settings — and `setDescription` hands the header
 * title's tooltip its one line. `null` outside the shell, so a `PageHeader`
 * rendered on its own (every feature test) falls back to rendering its
 * `actions` inline instead of reaching for a slot that does not exist.
 */
export interface PageHeaderSlot {
  actionsSlot: HTMLDivElement | null
  setDescription: (description: ReactNode) => void
}

export const PageHeaderContext = createContext<PageHeaderSlot | null>(null)
