/**
 * What every test in this package is given before its module is imported.
 *
 * `daemonFetch` is the daemon stub `dashboard-client.ts` sees — configured
 * per test, reset between them, exactly as the host app's own
 * `ui/src/test/setup.ts` does it. There is no Perses or MUI shim here: this
 * package's own tests are the glue (`page-params.ts`, `use-host-messages.ts`,
 * `dashboard-client.ts`, `datasource-api.ts`), never `page.tsx` itself.
 */

import { cleanup } from "@testing-library/react"
import { afterEach, beforeEach, vi } from "vitest"

export const daemonFetch = vi.fn()
globalThis.fetch = daemonFetch as unknown as typeof fetch

beforeEach(() => {
  daemonFetch.mockImplementation(() => new Promise(() => {}))
})

afterEach(() => {
  if (typeof window !== "undefined") cleanup()
  daemonFetch.mockReset()
  vi.unstubAllGlobals()
})
