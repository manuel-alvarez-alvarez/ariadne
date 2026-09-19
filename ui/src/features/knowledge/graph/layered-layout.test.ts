/**
 * The layered layout (022): ELK runs for real, on the calling thread instead
 * of in a web worker (`@/test/elk-worker.ts`), and what is asserted is where
 * it put each node: left to right, in the layer the model named.
 */

import { describe, expect, it, vi } from "vitest"

import { emptyGraph } from "./graph-model"
import { layeredLayout } from "./layered-layout"

vi.mock("elkjs/lib/elk-worker.min.js?worker", () => import("@/test/elk-worker"))

function x(graph: Awaited<ReturnType<typeof layeredLayout>>, node: string): number {
  return graph.getNodeAttribute(node, "x") ?? Number.NaN
}

describe("layeredLayout", () => {
  it("puts each node in the layer the model named, left to right", async () => {
    const graph = emptyGraph()
    graph.addNode("changed", { label: "changed", tone: "active", layer: 0 })
    graph.addNode("near", { label: "near", tone: "pending", layer: 1 })
    graph.addNode("far", { label: "far", tone: "pending", layer: 2 })
    graph.addNode("also-near", { label: "also-near", tone: "pending", layer: 1 })

    const laid = await layeredLayout(graph)

    expect(x(laid, "changed")).toBeLessThan(x(laid, "near"))
    expect(x(laid, "near")).toBeLessThan(x(laid, "far"))
    expect(x(laid, "also-near")).toBe(x(laid, "near"))
    // Two nodes of one layer do not sit on each other.
    expect(laid.getNodeAttribute("also-near", "y")).not.toBe(laid.getNodeAttribute("near", "y"))
  })

  it("puts a node with no layer where its edges lead, and leaves the model as it was", async () => {
    const graph = emptyGraph()
    for (const name of ["a", "b", "c"]) graph.addNode(name, { label: name, tone: "pending" })
    graph.addDirectedEdge("a", "b", { tone: "ready" })
    graph.addDirectedEdge("b", "c", { tone: "ready" })

    const laid = await layeredLayout(graph)

    expect(x(laid, "a")).toBeLessThan(x(laid, "b"))
    expect(x(laid, "b")).toBeLessThan(x(laid, "c"))
    expect(graph.getNodeAttribute("a", "x")).toBeUndefined()
  })
})
