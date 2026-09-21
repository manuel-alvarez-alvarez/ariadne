// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react"
import { beforeEach, describe, expect, it } from "vitest"

import { useCollapsedLanes } from "./collapsed-lanes"

beforeEach(() => localStorage.clear())

describe("useCollapsedLanes", () => {
  it("reads old saved lanes and keeps an explicit expansion", () => {
    localStorage.setItem("ariadne.goals.collapsed-lanes", '["finished"]')
    const { result } = renderHook(useCollapsedLanes)

    expect(result.current.isCollapsed("finished", false)).toBe(true)
    act(() => result.current.setCollapsed("finished", false))

    expect(result.current.isCollapsed("finished", true)).toBe(false)
    expect(localStorage.getItem("ariadne.goals.collapsed-lanes")).toBe(
      '{"collapsed":[],"expanded":["finished"]}',
    )
  })

  it("uses the board default until the user overrides it", () => {
    const { result } = renderHook(useCollapsedLanes)

    expect(result.current.isCollapsed("active", false)).toBe(false)
    act(() => result.current.setCollapsed("active", true))
    expect(result.current.isCollapsed("active", false)).toBe(true)
  })
})
