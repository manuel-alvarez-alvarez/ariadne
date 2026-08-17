import { describe, expect, it } from "vitest"

import { ApiError } from "@/api"
import { shouldRetryQuery } from "./query-client"

describe("shouldRetryQuery", () => {
  it("gives up at once on a daemon that cannot be reached", () => {
    // Every screen is looking at the same dead daemon, so they all have to
    // give up together — the connection banner is what says why.
    const down = ApiError.network(new Error("connection refused"))
    expect(shouldRetryQuery(0, down)).toBe(false)
  })

  it("gives up at once on a 4xx, which will not fix itself", () => {
    expect(
      shouldRetryQuery(0, new ApiError({ status: 404, code: "task_not_found", message: "gone" })),
    ).toBe(false)
  })

  it("retries a 5xx twice, which is transient", () => {
    const flaky = new ApiError({ status: 503, code: "http_error", message: "503 Unavailable" })
    expect(shouldRetryQuery(0, flaky)).toBe(true)
    expect(shouldRetryQuery(1, flaky)).toBe(true)
    expect(shouldRetryQuery(2, flaky)).toBe(false)
  })

  it("retries anything that is not an ApiError at all", () => {
    expect(shouldRetryQuery(0, new Error("boom"))).toBe(true)
    expect(shouldRetryQuery(2, new Error("boom"))).toBe(false)
  })
})
