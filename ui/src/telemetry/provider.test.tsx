// @vitest-environment jsdom

import { render } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { DEFAULT_BASE_URL } from "@/api"
import { useSettingsStore } from "@/stores/settings"
import { flushTelemetry } from "@/telemetry/telemetry"
import { daemonFetch } from "@/test/harness"
import { TelemetryProvider } from "./provider"

const native = vi.hoisted(() => ({
  enabled: false,
  onLog: undefined as
    | undefined
    | ((event: { payload: { level: number; message: string } }) => void),
}))
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => native.enabled }))
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, onLog: typeof native.onLog) => {
    native.onLog = onLog
    return () => {
      native.onLog = undefined
    }
  }),
}))

beforeEach(() => {
  useSettingsStore.setState({ baseUrl: "http://daemon.test", diagnosticsEnabled: true })
  daemonFetch.mockResolvedValue(new Response(null, { status: 200 }))
})

afterEach(() => {
  native.enabled = false
  useSettingsStore.setState({ baseUrl: DEFAULT_BASE_URL, diagnosticsEnabled: true })
  vi.restoreAllMocks()
})

describe("TelemetryProvider", () => {
  it("forwards native shell logs at their level when diagnostics are enabled", async () => {
    native.enabled = true
    daemonFetch.mockResolvedValue(new Response(null, { status: 200 }))
    render(
      <TelemetryProvider>
        <div />
      </TelemetryProvider>,
    )
    await vi.waitFor(() => expect(native.onLog).toBeDefined())
    native.onLog?.({ payload: { level: 3, message: "native window ready" } })
    await flushTelemetry()

    const request = daemonFetch.mock.calls
      .map(([input, init]) => (input instanceof Request ? input : new Request(String(input), init)))
      .find((request) => request.url.endsWith("/otlp/v1/logs"))
    const body = await request?.text()
    expect(body).toContain("native window ready")
    expect(body).toContain("tauri")
  })
  it("does not export raw malformed response frames from console errors", async () => {
    vi.useFakeTimers()
    vi.spyOn(console, "error").mockImplementation(() => {})
    render(
      <TelemetryProvider>
        <div />
      </TelemetryProvider>,
    )

    const rawResponse = '{"task":{"prompt":"do not export this response body"}}'
    console.error("[events] dropping unparseable payload", new Error("invalid JSON"), rawResponse)
    await vi.advanceTimersByTimeAsync(5_000)
    await flushTelemetry()

    const request = daemonFetch.mock.calls
      .map(([input, init]) => (input instanceof Request ? input : new Request(String(input), init)))
      .find((request) => request.url.endsWith("/otlp/v1/logs"))
    expect(request).toBeDefined()
    const body = await request?.text()
    expect(body).not.toContain("do not export this response body")
    expect(body).toContain("exception.stacktrace")
  })

  it("does not capture console warnings when diagnostics are disabled", async () => {
    useSettingsStore.setState({ diagnosticsEnabled: false })
    vi.spyOn(console, "warn").mockImplementation(() => {})
    render(
      <TelemetryProvider>
        <div />
      </TelemetryProvider>,
    )

    console.warn("daemon response body must stay local")
    await flushTelemetry()

    expect(daemonFetch).not.toHaveBeenCalled()
  })
})
