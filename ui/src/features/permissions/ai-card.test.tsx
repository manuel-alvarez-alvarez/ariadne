// @vitest-environment jsdom

/**
 * `AiCard`'s own wiring between the test panel and the range control: the
 * marker a test's danger draws on the track, and how a threshold moved
 * afterwards relabels that same danger with no second call to
 * `/v1/permissions/ai/test` — plus the header's own button group, since that
 * markup is `AiCard`'s. Everything else about the card is covered by
 * `permissions-page.test.tsx` (the AI tab), and each control has its own file
 * (`threshold-range.test.tsx`, `ai-test-panel.test.tsx`).
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { AiPermissionsStatusDto, TestAiPermissionResponse } from "@/api"
import { TooltipProvider } from "@/components/ui/tooltip"
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
      <TooltipProvider delay={0}>
        <AiCard status={status} />
      </TooltipProvider>
    </QueryClientProvider>,
  )
  return {
    rerender: (next: AiPermissionsStatusDto) =>
      view.rerender(
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delay={0}>
            <AiCard status={next} />
          </TooltipProvider>
        </QueryClientProvider>,
      ),
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
  renderCard(status)

  expect(screen.queryByLabelText("Tool")).toBeNull()
  await user.click(screen.getByRole("button", { name: "Test a request" }))
  expect(await screen.findByRole("dialog")).toBeDefined()
  await user.click(screen.getByRole("button", { name: "Test" }))

  expect(await screen.findByLabelText("Danger 0.5000")).toBeDefined()
  expect(await screen.findByText("Ask", { selector: '[data-slot="badge"]' })).toBeDefined()

  await user.click(screen.getByRole("button", { name: "Close" }))
  expect(screen.queryByRole("dialog")).toBeNull()
  expect(screen.getByLabelText("Danger 0.5000")).toBeDefined()
})

it("relabels the same danger once a threshold moves, with no second call", async () => {
  const status = anAiPermissionsStatus({ enabled: true, allow_threshold: 0.2, deny_threshold: 0.8 })
  const user = userEvent.setup()
  const { rerender } = renderCard(status)

  await user.click(screen.getByRole("button", { name: "Test a request" }))
  await user.click(await screen.findByRole("button", { name: "Test" }))
  expect(await screen.findByText("Ask", { selector: '[data-slot="badge"]' })).toBeDefined()
  const callsAfterTest = requests.filter((one) => one.path === "/v1/permissions/ai/test").length

  // The same move `ThresholdRange`'s own commit would make, applied the way
  // its caller (here, and `PermissionsPage` for real) applies it: as a new
  // `status` prop, once the write it sent has settled.
  rerender(anAiPermissionsStatus({ enabled: true, allow_threshold: 0.6, deny_threshold: 0.8 }))

  expect(await screen.findByText("Allow", { selector: '[data-slot="badge"]' })).toBeDefined()
  expect(requests.filter((one) => one.path === "/v1/permissions/ai/test").length).toBe(
    callsAfterTest,
  )
})

it("puts Test a request and Refresh in one button group in the header", () => {
  const status = anAiPermissionsStatus({ enabled: true, state: "ready" })
  renderCard(status)

  const testButton = screen.getByRole("button", { name: "Test a request" })
  const refreshButton = screen.getByRole("button", { name: "Refresh" })
  const group = testButton.closest('[data-slot="button-group"]')

  expect(group).not.toBeNull()
  expect(group?.contains(refreshButton)).toBe(true)
  expect(testButton.closest("header")?.contains(group)).toBe(true)
})

it("renders the header buttons icon-only, with a tooltip naming each", async () => {
  const status = anAiPermissionsStatus({ enabled: true, state: "ready" })
  const user = userEvent.setup()
  renderCard(status)

  const testButton = screen.getByRole("button", { name: "Test a request" })
  const refreshButton = screen.getByRole("button", { name: "Refresh" })
  expect(testButton.textContent).toBe("")
  expect(refreshButton.textContent).toBe("")

  await user.tab() // the Enable switch, first in the header row
  await user.tab()
  expect(document.activeElement).toBe(testButton)
  expect(await screen.findByText("Test a request")).toBeDefined()

  await user.tab()
  expect(document.activeElement).toBe(refreshButton)
  expect(await screen.findByText("Refresh")).toBeDefined()
})

it("opens the Details popover with the full seven facts", async () => {
  const status = anAiPermissionsStatus({
    state: "ready",
    installed_release: "kev@f1535963 jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101",
    weights_present: true,
    endpoint: "http://127.0.0.1:8901",
  })
  const user = userEvent.setup()
  renderCard(status)

  expect(screen.queryByText("http://127.0.0.1:8901")).toBeNull()
  await user.click(screen.getByRole("button", { name: "Details" }))

  expect(screen.getByText("http://127.0.0.1:8901")).toBeDefined()
  expect(screen.getByText("Yes")).toBeDefined()
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

  await user.click(screen.getByRole("button", { name: "Test a request" }))
  await user.click(await screen.findByRole("button", { name: "Test" }))

  expect(await screen.findByText("No answer: unavailable")).toBeDefined()
  expect(screen.queryByLabelText(/^Danger/)).toBeNull()
})
