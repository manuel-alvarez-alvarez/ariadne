// @vitest-environment jsdom

import { renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { emptyGraph } from "./graph-model"
import { useLayeredLayout } from "./layered-layout"

vi.mock("elkjs/lib/elk-worker.min.js?worker", () => import("@/test/elk-worker"))

describe("useLayeredLayout", () => {
  it("places named layers from left to right without changing the input graph", async () => {
    const graph = emptyGraph()
    graph.addNode("changed", { label: "changed", tone: "active", layer: 0 })
    graph.addNode("near", { label: "near", tone: "pending", layer: 1 })
    graph.addNode("far", { label: "far", tone: "pending", layer: 2 })

    const { result } = renderHook(() => useLayeredLayout(graph))

    await waitFor(() => expect(result.current.graph).toBeDefined())
    const laid = result.current.graph
    if (!laid) throw new Error("layout did not finish")
    expect(laid.getNodeAttribute("changed", "x")).toBeLessThan(
      laid.getNodeAttribute("near", "x") ?? Infinity,
    )
    expect(laid.getNodeAttribute("near", "x")).toBeLessThan(
      laid.getNodeAttribute("far", "x") ?? Infinity,
    )
    expect(graph.getNodeAttribute("changed", "x")).toBeUndefined()
  })

  it("uses directed edges to place nodes without named layers", async () => {
    const graph = emptyGraph()
    for (const name of ["a", "b", "c"]) graph.addNode(name, { label: name, tone: "pending" })
    graph.addDirectedEdge("a", "b", { tone: "ready" })
    graph.addDirectedEdge("b", "c", { tone: "ready" })

    const { result } = renderHook(() => useLayeredLayout(graph))

    await waitFor(() => expect(result.current.graph).toBeDefined())
    const laid = result.current.graph
    if (!laid) throw new Error("layout did not finish")
    expect(laid.getNodeAttribute("a", "x")).toBeLessThan(
      laid.getNodeAttribute("b", "x") ?? Infinity,
    )
    expect(laid.getNodeAttribute("b", "x")).toBeLessThan(
      laid.getNodeAttribute("c", "x") ?? Infinity,
    )
  })
})
