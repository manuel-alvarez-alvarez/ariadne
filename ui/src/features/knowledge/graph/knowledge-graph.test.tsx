// @vitest-environment jsdom

/**
 * The shared knowledge graph (022): the model it places, the emphasis a hover
 * gives, the colours it reads off the app's tokens, and the node and edge
 * clicks it hands back.
 *
 * sigma.js needs WebGL, so this file draws the graph with a stand-in
 * that lists each node and edge the way the component's reducers describe
 * it (`@/test/sigma-canvas.tsx`): what is asserted is what sigma would be
 * told to draw, not pixels.
 */

import { fireEvent, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import { renderScreen } from "@/test/harness"

import { emphasis, emptyGraph, type KnowledgeGraphModel, placed, settled } from "./graph-model"
import { KnowledgeGraph } from "./knowledge-graph"

// jsdom has no WebGL, and sigma.js reads `WebGLRenderingContext` as its module
// loads: the graph is drawn by the stand-in in `@/test/sigma-canvas.tsx`. It is
// mocked here, in the files that draw a graph, and not in `@/test/setup`: a
// mock in the setup file slows every test file in the suite.
vi.mock("@/features/knowledge/graph/sigma-canvas", () => import("@/test/sigma-canvas"))

/** a → b → c, and d on its own. */
function aChain(): KnowledgeGraphModel {
  const graph = emptyGraph()
  graph.addNode("a", { label: "alpha", tone: "pending" })
  graph.addNode("b", { label: "beta", tone: "pending" })
  graph.addNode("c", { label: "gamma", tone: "pending" })
  graph.addNode("d", { label: "delta", tone: "pending" })
  graph.addDirectedEdgeWithKey("a-b", "a", "b", { label: "2", tone: "active" })
  graph.addDirectedEdgeWithKey("b-c", "b", "c", { label: "1", tone: "warn", dashed: true })
  return graph
}

function node(name: string): HTMLElement {
  return within(screen.getByRole("list", { name: "Graph nodes" })).getByRole("button", { name })
}

function edge(key: string): HTMLElement {
  const found = document.querySelector<HTMLElement>(`[data-edge="${key}"]`)
  if (!found) throw new Error(`no edge ${key}`)
  return found
}

afterEach(() => {
  document.documentElement.removeAttribute("style")
})

describe("the model", () => {
  it("places every node the model left unplaced, and keeps the ones it placed", () => {
    const graph = aChain()
    graph.mergeNodeAttributes("a", { x: 5, y: 7 })

    const shown = placed(graph)

    expect(shown.getNodeAttributes("a")).toMatchObject({ x: 5, y: 7 })
    for (const key of ["b", "c", "d"]) {
      expect(typeof shown.getNodeAttribute(key, "x")).toBe("number")
      expect(typeof shown.getNodeAttribute(key, "y")).toBe("number")
    }
    // A copy: the caller's model is left as it was built.
    expect(graph.getNodeAttribute("b", "x")).toBeUndefined()
  })

  it("keeps a hovered node and its neighbours, both ways, and fades the rest", () => {
    const hover = emphasis(aChain(), "b")

    expect(hover.node("a")).toEqual({ highlighted: true, faded: false })
    expect(hover.node("b")).toEqual({ highlighted: true, faded: false })
    expect(hover.node("c")).toEqual({ highlighted: true, faded: false })
    expect(hover.node("d")).toEqual({ highlighted: false, faded: true })
  })

  it("keeps the edges that touch the hovered node and fades the others", () => {
    const hover = emphasis(aChain(), "a")

    expect(hover.edge("a-b")).toEqual({ highlighted: true, faded: false })
    expect(hover.edge("b-c")).toEqual({ highlighted: false, faded: true })
  })

  it("calls a layout settled once its nodes barely move, and not while they still travel", () => {
    // Two nodes, ten units apart.
    const before = new Float64Array([0, 0, 10, 0])

    expect(settled(before, new Float64Array([0.001, 0, 10, 0]))).toBe(true)
    expect(settled(before, new Float64Array([1, 0, 10, 0]))).toBe(false)
  })

  it("neither keeps nor fades anything while nothing is hovered", () => {
    const hover = emphasis(aChain(), null)

    expect(hover.node("d")).toEqual({ highlighted: false, faded: false })
    expect(hover.edge("b-c")).toEqual({ highlighted: false, faded: false })
  })
})

describe("the view", () => {
  it("draws one node per model node, labelled, and one edge per model edge", () => {
    renderScreen(<KnowledgeGraph graph={aChain()} layout="force" legend={[]} label="Chain" />)

    const nodes = within(screen.getByRole("list", { name: "Graph nodes" })).getAllByRole("button")
    expect(nodes.map((button) => button.textContent)).toEqual(["alpha", "beta", "gamma", "delta"])
    expect(edge("a-b").textContent).toBe("alpha → beta")
    expect(edge("a-b").dataset.label).toBe("2")
    expect(screen.getByTestId("graph-canvas").dataset.layout).toBe("force")
  })

  it("draws a dashed edge as dashed and the rest as lines", () => {
    renderScreen(<KnowledgeGraph graph={aChain()} layout="circle" legend={[]} label="Chain" />)

    expect(edge("b-c").dataset.type).toBe("dashed")
    expect(edge("a-b").dataset.type).toBe("line")
  })

  it("colours nodes, edges and the legend from the status tokens of the theme", () => {
    const style = document.documentElement.style
    style.setProperty("--status-pending", "oklch(0 0 0)")
    style.setProperty("--status-active", "oklch(1 0 0)")

    renderScreen(
      <KnowledgeGraph
        graph={aChain()}
        layout="force"
        legend={[{ label: "Depends on", tone: "active" }]}
        label="Chain"
      />,
    )

    expect(node("alpha").dataset.color).toBe("#000000")
    expect(edge("a-b").dataset.color).toBe("#ffffff")
    const legend = screen.getByRole("list", { name: "Legend" })
    expect(legend.textContent).toBe("Depends on")
    expect(legend.querySelector<HTMLElement>("[data-swatch]")?.dataset.swatch).toBe("#ffffff")
  })

  it("keeps the hovered node's neighbourhood and fades the rest until the pointer leaves", () => {
    renderScreen(<KnowledgeGraph graph={aChain()} layout="force" legend={[]} label="Chain" />)
    const unfaded = node("delta").dataset.color

    fireEvent.mouseEnter(node("alpha"))

    expect(node("alpha").dataset.highlighted).toBe("true")
    expect(node("beta").dataset.highlighted).toBe("true")
    expect(node("delta").dataset.label).toBe("")
    expect(node("delta").dataset.color).not.toBe(unfaded)
    expect(edge("b-c").dataset.label).toBe("")

    fireEvent.mouseLeave(node("alpha"))

    expect(node("delta").dataset.label).toBe("delta")
    expect(node("delta").dataset.color).toBe(unfaded)
  })

  it("leaves out the nodes and edges it is told to hide, and draws the rest", () => {
    renderScreen(
      <KnowledgeGraph
        graph={aChain()}
        layout="force"
        legend={[]}
        label="Chain"
        hidden={{ nodes: new Set(["d"]), edges: new Set(["b-c"]) }}
      />,
    )

    const nodes = within(screen.getByRole("list", { name: "Graph nodes" })).getAllByRole("button")
    expect(nodes.map((button) => button.textContent)).toEqual(["alpha", "beta", "gamma"])
    expect(edge("b-c").closest("li")?.hidden).toBe(true)
    expect(edge("a-b").closest("li")?.hidden).toBe(false)
  })

  it("hands a clicked node and a clicked edge back by their keys", async () => {
    const user = userEvent.setup()
    const onNodeClick = vi.fn()
    const onEdgeClick = vi.fn()
    renderScreen(
      <KnowledgeGraph
        graph={aChain()}
        layout="force"
        legend={[]}
        label="Chain"
        onNodeClick={onNodeClick}
        onEdgeClick={onEdgeClick}
      />,
    )

    await user.click(node("gamma"))
    await user.click(edge("b-c"))

    expect(onNodeClick).toHaveBeenCalledWith("c")
    expect(onEdgeClick).toHaveBeenCalledWith("b-c")
  })
})
