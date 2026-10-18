// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest"

import { daemonFetch } from "@/test/harness"

import {
  configureTelemetry,
  flushTelemetry,
  recordApiRequest,
  recordLog,
  recordNavigation,
} from "./telemetry"

describe("Telemetry", () => {
  afterEach(() => vi.restoreAllMocks())

  it("posts OTLP logs and metrics with the desktop resource", async () => {
    vi.useFakeTimers()
    daemonFetch.mockResolvedValue(new Response(null, { status: 200 }))
    configureTelemetry({ baseUrl: "http://daemon.test", enabled: true })

    recordLog("warn", "slow request")
    recordApiRequest("GET", "/v1/goals", 200, 42)
    await vi.advanceTimersByTimeAsync(5_000)
    await flushTelemetry()

    const requests = daemonFetch.mock.calls.map(([input, init]) =>
      input instanceof Request ? input : new Request(String(input), init),
    )
    expect(requests.map((request) => request.url)).toEqual(
      expect.arrayContaining([
        "http://daemon.test/otlp/v1/logs",
        "http://daemon.test/otlp/v1/metrics",
      ]),
    )
    const payloads = await Promise.all(requests.map(async (request) => request.json()))
    expect(payloads).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ resourceLogs: expect.any(Array) }),
        expect.objectContaining({ resourceMetrics: expect.any(Array) }),
      ]),
    )
    expect(JSON.stringify(payloads)).toContain("service.name")
    expect(JSON.stringify(payloads)).toContain("ariadne-desktop")
    expect(JSON.stringify(payloads)).toContain("ariadne.ui.api.request.duration")
  })

  it("does not export when diagnostics are disabled", async () => {
    const fetch = daemonFetch
    configureTelemetry({ baseUrl: "http://daemon.test", enabled: false })

    recordLog("error", "failed")
    recordNavigation("/goals")
    await flushTelemetry()

    expect(fetch).not.toHaveBeenCalled()
  })

  it("prints one debug line when a background metric export fails", async () => {
    vi.useFakeTimers()
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {})
    daemonFetch.mockResolvedValue(new Response(null, { status: 400 }))
    configureTelemetry({ baseUrl: "http://daemon.test", enabled: true })

    recordNavigation("/goals")
    await vi.advanceTimersByTimeAsync(5_000)

    expect(debug).toHaveBeenCalledWith("[telemetry] export failed")
  })
})
