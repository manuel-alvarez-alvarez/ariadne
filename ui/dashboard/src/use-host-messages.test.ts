// @vitest-environment jsdom

import { renderHook } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { useHostMessages } from "./use-host-messages"

function postFromHost(data: unknown, origin = window.location.origin): void {
  window.dispatchEvent(new MessageEvent("message", { data, origin }))
}

describe("useHostMessages", () => {
  it("calls onTheme for a same-origin theme message", () => {
    const onTheme = vi.fn()
    renderHook(() => useHostMessages({ onTheme, onReload: vi.fn() }))

    postFromHost({ type: "theme", theme: "dark" })

    expect(onTheme).toHaveBeenCalledWith("dark")
  })

  it("calls onReload for a same-origin reload message", () => {
    const onReload = vi.fn()
    renderHook(() => useHostMessages({ onTheme: vi.fn(), onReload }))

    postFromHost({ type: "reload" })

    expect(onReload).toHaveBeenCalledOnce()
  })

  it("ignores a message from another origin", () => {
    const onTheme = vi.fn()
    renderHook(() => useHostMessages({ onTheme, onReload: vi.fn() }))

    postFromHost({ type: "theme", theme: "dark" }, "https://evil.example")

    expect(onTheme).not.toHaveBeenCalled()
  })

  it("ignores a message with no recognized type", () => {
    const onTheme = vi.fn()
    const onReload = vi.fn()
    renderHook(() => useHostMessages({ onTheme, onReload }))

    postFromHost("just a string")
    postFromHost({ type: "unknown" })

    expect(onTheme).not.toHaveBeenCalled()
    expect(onReload).not.toHaveBeenCalled()
  })

  it("stops listening once unmounted", () => {
    const onReload = vi.fn()
    const { unmount } = renderHook(() => useHostMessages({ onTheme: vi.fn(), onReload }))
    unmount()

    postFromHost({ type: "reload" })

    expect(onReload).not.toHaveBeenCalled()
  })
})
