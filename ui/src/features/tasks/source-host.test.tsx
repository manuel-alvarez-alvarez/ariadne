// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { renderHook, waitFor } from "@testing-library/react"
import { expect, it } from "vitest"
import { LOCAL, stubHosts } from "@/test/hosts"
import { useDaemonIdentities, useSourceHost } from "./source-host"

function wrapper({ children }: { children: React.ReactNode }) {
  return <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>
}

it("keeps source resolution pending while the hosts load", () => {
  stubHosts({ local: { "/v1/hosts": () => undefined } })
  const { result } = renderHook(() => useSourceHost("source"), { wrapper })
  expect(result.current).toEqual({ host: undefined, isPending: true })
})

it("resolves an unknown source only after all identities answer", async () => {
  stubHosts(
    { local: { "/v1/remote-tasks/protocol": { protocol: "remote-tasks-v1", daemon_id: "other" } } },
    [LOCAL],
  )
  const { result } = renderHook(
    () => ({ source: useSourceHost("source"), identities: useDaemonIdentities(true) }),
    { wrapper },
  )
  expect(result.current.source.isPending).toBe(true)
  await waitFor(() => expect(result.current.source).toEqual({ host: undefined, isPending: false }))
  expect(result.current.identities.isPending).toBe(false)
  expect(result.current.identities.get("local")).toBe("other")
})
