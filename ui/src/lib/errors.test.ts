import { describe, expect, it } from "vitest"

import { ApiError } from "@/api"
import { describeError } from "./errors"

describe("describeError", () => {
  it("appends the daemon's error code, which is what its docs talk about", () => {
    const error = new ApiError({
      status: 409,
      code: "illegal_transition",
      message: "cannot cancel",
    })
    expect(describeError(error)).toBe("cannot cancel (illegal_transition)")
  })

  it("leaves off the codes that name nothing a reader could look up", () => {
    expect(describeError(ApiError.network(new Error("connection refused")))).toBe(
      "cannot reach the daemon: connection refused",
    )
    expect(
      describeError(new ApiError({ status: 502, code: "http_error", message: "502 Bad Gateway" })),
    ).toBe("502 Bad Gateway")
  })

  it("falls back to whatever was thrown", () => {
    expect(describeError(new Error("boom"))).toBe("boom")
    expect(describeError("boom")).toBe("boom")
  })
})
