// @vitest-environment jsdom

/**
 * `AiCard`'s own wiring between the test panel and the range control: the
 * marker a test's danger draws on the track, and how a threshold moved
 * afterwards relabels that same danger with no second call to
 * `/v1/permissions/ai/test`. Everything else about the card is covered by
 * `permissions-page.test.tsx` (the AI tab), and each control has its own file
 * (`threshold-range.test.tsx`, `ai-test-panel.test.tsx`).
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { AiPermissionsStatusDto, TestAiPermissionResponse } from "@/api"
import { anAiPermissionsStatus } from "@/test/fixtures"
import { daemonFetch, jsonResponse } from "@/test/harness"
import { AiCard } from "./ai-card"

let requests: { method: string; path: string }[] = []
let testResponse: TestAiPermissionResponse

function stubDaemon() {
  requests = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    requests.push({ method: request.method, path: pathname })
    if (pathname === "/v1/permissions/ai/test") return jsonResponse(testResponse)
    throw new Error(`unhandled request ${request.method} ${pathname}`)
  })
}

function renderCard(status: AiPermissionsStatusDto) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <AiCard status={status} />
    </QueryClientProvider>,
  )
  return {
    rerender: (next: AiPermissionsStatusDto) =>
      view.rerender(
        <QueryClientProvider client={queryClient}>
          <AiCard status={next} />
        </QueryClientProvider>,
      ),
    /** The result badge's own text — distinct from the track's plain "Ask" label. */
    badgeText: () => view.container.querySelector('[data-slot="badge"]')?.textContent,
  }
}

beforeEach(() => {
  testResponse = {
    label: "ask",
    danger: 0.5,
    allow_threshold: 0.2,
    deny_threshold: 0.8,
    ai_error: null,
  }
  stubDaemon()
})

it("draws the marker and the label from a test result", async () => {
  const status = anAiPermissionsStatus({ enabled: true, allow_threshold: 0.2, deny_threshold: 0.8 })
  const user = userEvent.setup()
  const { badgeText } = renderCard(status)

  await user.click(screen.getByRole("button", { name: "Test" }))

  expect(await screen.findByLabelText("Danger 0.5")).toBeDefined()
  await waitFor(() => expect(badgeText()).toBe("Ask"))
})

it("relabels the same danger once a threshold moves, with no second call", async () => {
  const status = anAiPermissionsStatus({ enabled: true, allow_threshold: 0.2, deny_threshold: 0.8 })
  const user = userEvent.setup()
  const { rerender, badgeText } = renderCard(status)

  await user.click(screen.getByRole("button", { name: "Test" }))
  await waitFor(() => expect(badgeText()).toBe("Ask"))
  const callsAfterTest = requests.filter((one) => one.path === "/v1/permissions/ai/test").length

  // The same move `ThresholdRange`'s own commit would make, applied the way
  // its caller (here, and `PermissionsPage` for real) applies it: as a new
  // `status` prop, once the write it sent has settled.
  rerender(anAiPermissionsStatus({ enabled: true, allow_threshold: 0.6, deny_threshold: 0.8 }))

  await waitFor(() => expect(badgeText()).toBe("Allow"))
  expect(requests.filter((one) => one.path === "/v1/permissions/ai/test").length).toBe(
    callsAfterTest,
  )
})

it("draws no marker for an ai_error answer", async () => {
  testResponse = {
    label: null,
    danger: null,
    allow_threshold: 0.2,
    deny_threshold: 0.8,
    ai_error: "unavailable",
  }
  const status = anAiPermissionsStatus({ enabled: true, allow_threshold: 0.2, deny_threshold: 0.8 })
  const user = userEvent.setup()
  renderCard(status)

  await user.click(screen.getByRole("button", { name: "Test" }))

  expect(await screen.findByText("No answer: unavailable")).toBeDefined()
  expect(screen.queryByLabelText(/^Danger/)).toBeNull()
})
