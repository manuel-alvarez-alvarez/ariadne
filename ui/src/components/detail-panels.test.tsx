// @vitest-environment jsdom

/**
 * Focus across the panel stack, driven the way the app drives it: by the URL.
 *
 * `DetailPanels` is the piece that decides which panels are open and which of
 * them owns `?session=`, and that decision is exactly what focus depends on —
 * so the real thing is mounted rather than a stand-in for it. The daemon never
 * answers, which is enough: every panel has a pending state, and none of them
 * needs data to be a dialog with focus in it.
 *
 * The case is the one the goal panel's own params make easy to get wrong. A
 * goal drilled into a session, followed to the task that session ran, opens a
 * second sheet on top — and the goal panel's `?session=` goes away in the same
 * navigation, which is *not* the user coming back out of that session. Focus
 * belongs to the sheet on top; see `hooks/use-focus-return.ts`.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import { MemoryRouter, type NavigateFunction, useNavigate } from "react-router-dom"
import { afterEach, expect, it, vi } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { paths } from "@/routes/paths"

import { DetailPanels } from "./detail-panels"

/**
 * Hoisted, and not `vi.stubGlobal`: `openapi-fetch` takes its `fetch` when the
 * client is built, which is when `@/api` is imported — a stub installed after
 * that is one the daemon client never sees.
 *
 * It never settles, so every panel stays in the pending state it renders
 * without data. That is the point: this is about focus, not about content.
 */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})
daemonFetch.mockImplementation(() => new Promise(() => {}))

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(cleanup)

/** The router's `navigate`, for URL changes no rendered control stands in for. */
let go: NavigateFunction | undefined
function Navigator() {
  go = useNavigate()
  return null
}

function mountPanels(at: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <TooltipProvider delay={0}>
        <MemoryRouter initialEntries={[at]}>
          <Navigator />
          <DetailPanels />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  )
}

/** The sheet a given title belongs to — the stack is two of them at once. */
function sheetTitled(title: string): HTMLElement {
  const sheet = screen.getByText(title).closest('[data-slot="sheet-content"]')
  if (!sheet) throw new Error(`no sheet holding "${title}"`)
  return sheet as HTMLElement
}

it("leaves focus in the task sheet stacked over a goal's session", async () => {
  mountPanels(`${paths.goals()}?goal=g1&tab=sessions&session=s1`)
  const goalSheet = sheetTitled("Session s1")
  // The dialog takes focus of its own accord, a tick after it mounts.
  await waitFor(() => expect(goalSheet.contains(document.activeElement)).toBe(true))

  // The Task link of a session shown inside a goal: `taskPanelTo` adds `?task=`
  // and drops the panel's own `tab`/`session` in the one navigation.
  await act(async () => {
    go?.({ pathname: paths.goals(), search: "?goal=g1&task=t1" })
  })

  // On the spot, and about the panel underneath: the sheet on top asserts its
  // own focus a tick later, which would paper over the goal panel reading this
  // navigation as "came back out of the session" and grabbing focus first.
  expect(goalSheet.contains(document.activeElement)).toBe(false)

  const taskSheet = sheetTitled("Loading task")
  expect(taskSheet).not.toBe(goalSheet)
  await waitFor(() => expect(taskSheet.contains(document.activeElement)).toBe(true))
})
