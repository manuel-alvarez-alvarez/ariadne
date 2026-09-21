// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react"
import { beforeEach, describe, expect, it } from "vitest"

import { useDiffWrap } from "./diff-prefs"

beforeEach(() => localStorage.clear())

describe("useDiffWrap", () => {
  it("defaults to wrapping and saves each user choice", () => {
    const { result } = renderHook(useDiffWrap)

    expect(result.current.wrap).toBe(true)
    act(() => result.current.setWrap(false))
    expect(result.current.wrap).toBe(false)
    expect(localStorage.getItem("ariadne.tasks.diff-wrap")).toBe("off")
  })
})
