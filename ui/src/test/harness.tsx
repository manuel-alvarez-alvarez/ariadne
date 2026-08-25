/**
 * Rendering a screen the way the app does, and answering it as the daemon.
 *
 * Every component test needs the same three things around whatever it is
 * testing — a query client that does not retry and does not cache between
 * tests, a router for the links and search params the screen reads, and a
 * tooltip provider that does not make a test wait for a hover delay — and every
 * one of them was building all three itself. Written twenty times they drifted:
 * a different `gcTime`, a router in one test of a screen and not in the next.
 *
 * The daemon is a `fetch` stub installed in `./setup` before anything imports
 * `@/api`. What it answers is the test's own business — that is the part that
 * differs — so this only provides the shapes: a JSON body, and the daemon's
 * error envelope.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, useLocation } from "react-router-dom"
import { TooltipProvider } from "@/components/ui/tooltip"

export { daemonFetch } from "./setup"

/** Where the router is, kept up to date by the probe `renderScreen` mounts. */
interface Location {
  /** Pathname and search, as a link would spell them: `/goals?goal=01J…`. */
  url: string
}

/**
 * The screen, under the providers it runs under in the app.
 *
 * `retry: false` so a stubbed failure is one request and one assertion rather
 * than three backoffs, and `gcTime: 0` so nothing one test cached is still
 * around for the next.
 */
export function renderScreen(
  ui: ReactNode,
  {
    route = "/",
    seed,
  }: {
    /**
     * The URL the screen is opened at, read back through the returned location.
     * `null` for a tree that brings its own router.
     */
    route?: string | null
    /**
     * Cache entries the screen should find already there. Seeding also pins the
     * data: a fixture nobody stubbed a request for must not be refetched out
     * from under the assertions.
     */
    seed?: (queryClient: QueryClient) => void
  } = {},
): { queryClient: QueryClient; location: Location; rerender: (next: ReactNode) => void } {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: seed ? Number.POSITIVE_INFINITY : 0 },
      mutations: { retry: false },
    },
  })
  seed?.(queryClient)
  const location: Location = { url: route ?? "/" }
  function Probe() {
    const current = useLocation()
    location.url = `${current.pathname}${current.search}`
    return null
  }
  const wrap = (children: ReactNode) => {
    const providers = (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delay={0}>
          {children}
          {route === null ? null : <Probe />}
        </TooltipProvider>
      </QueryClientProvider>
    )
    return route === null ? (
      providers
    ) : (
      <MemoryRouter initialEntries={[route]}>{providers}</MemoryRouter>
    )
  }
  const view = render(wrap(ui))
  return { queryClient, location, rerender: (next) => view.rerender(wrap(next)) }
}

/** A 2xx body, as the daemon sends it. */
export function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  })
}

/** The envelope every non-2xx carries: `{"error": {"code", "message"}}`. */
export function errorResponse(status: number, code: string, message: string): Response {
  return new Response(JSON.stringify({ error: { code, message } }), {
    status,
    headers: { "content-type": "application/json" },
  })
}
