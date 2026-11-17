// @vitest-environment jsdom

/** The one rule for when a row or a panel names its host. */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { renderHook, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { afterEach, beforeEach, expect, it } from "vitest"

import { useHosts } from "@/api"
import { useSettingsStore } from "@/stores/settings"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { usePanelShowsHost, useRowsShowHost } from "./use-host-label"

function wrapper({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
}

/** Both answers of the rule, once the settings daemon has listed its hosts. */
async function rule(pick: string, hosts = [LOCAL, BOX]) {
  useSettingsStore.setState({ hostFilter: pick })
  stubHosts({}, hosts)
  const { result } = renderHook(
    () => ({
      listed: !useHosts().isPending,
      rows: useRowsShowHost(),
      panel: usePanelShowsHost(),
    }),
    { wrapper },
  )
  // `local` alone is also the answer before the list lands, so wait for it.
  await waitFor(() => expect(result.current.listed).toBe(true))
  return { rows: result.current.rows, panel: result.current.panel }
}

beforeEach(() => useSettingsStore.setState({ hostFilter: "" }))
afterEach(() => {
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

it("shows the host on rows and panels under All with a named host listed", async () => {
  expect(await rule("")).toEqual({ rows: true, panel: true })
})

it("shows no host on rows under one picked host, but still on panels", async () => {
  expect(await rule("box")).toEqual({ rows: false, panel: true })
})

it("shows no host on rows when local is the picked host", async () => {
  expect(await rule("local")).toEqual({ rows: false, panel: true })
})

it("keeps the host on rows when a named host goes down", async () => {
  const down = { ...BOX, status: "down" as const, error: "connection refused" }
  expect(await rule("", [LOCAL, down])).toEqual({ rows: true, panel: true })
})

it("counts a saved pick the daemon no longer lists as All", async () => {
  expect(await rule("gone")).toEqual({ rows: true, panel: true })
})

it("shows no host on rows or panels with only local listed", async () => {
  expect(await rule("", [LOCAL])).toEqual({ rows: false, panel: false })
})
