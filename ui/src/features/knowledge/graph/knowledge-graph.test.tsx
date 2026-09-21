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

import { forceLayout } from "./force-layout"
import { emphasis, emptyGraph, type KnowledgeGraphModel, placed } from "./graph-model"
import { KnowledgeGraph } from "./knowledge-graph"

type SigmaCanvasProps = Parameters<typeof import("./sigma-canvas").SigmaCanvas>[0]

/**
 * The very graph the view was last handed, kept as it is: its nodes are
 * where the view has them now, and not where they were when it drew.
 */
const handed = vi.hoisted(() => ({ graph: null as KnowledgeGraphModel | null }))

// jsdom has no WebGL, and sigma.js reads `WebGLRenderingContext` as its module
// loads: the graph is drawn by the stand-in in `@/test/sigma-canvas.tsx`. It is
// mocked here, in the files that draw a graph, and not in `@/test/setup`: a
// mock in the setup file slows every test file in the suite.
vi.mock("@/features/knowledge/graph/sigma-canvas", async () => {
  const standIn = await import("@/test/sigma-canvas")
  return {
    SigmaCanvas: (props: SigmaCanvasProps) => {
      handed.graph = props.graph
      return standIn.SigmaCanvas(props)
    },
  }
})

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

/** Where a model has each of its nodes now, by key. */
function places(graph: KnowledgeGraphModel): Record<string, [number, number]> {
  const out: Record<string, [number, number]> = {}
  graph.forEachNode((node, attributes) => {
    out[node] = [attributes.x ?? Number.NaN, attributes.y ?? Number.NaN]
  })
  return out
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
    expect(screen.getByTestId("graph-canvas").dataset.layout).toBe("force")
  })

  it("hands the view the settled places, and lets nothing move them after", () => {
    vi.useFakeTimers()
    try {
      const graph = aChain()
      const laid = forceLayout(graph)
      renderScreen(<KnowledgeGraph graph={graph} layout="force" legend={[]} label="Chain" />)
      const drawn = handed.graph
      if (!drawn) throw new Error("the view was handed no graph")

      expect(places(drawn)).toEqual(places(laid))

      // Longer than the ten seconds the layout worker used to run for. The
      // model the view holds is read again, and not what it drew once, so a
      // timer or a worker that moved a node would show here.
      vi.advanceTimersByTime(30_000)
      fireEvent.mouseEnter(node("alpha"))
      fireEvent.mouseLeave(node("alpha"))

      expect(places(drawn)).toEqual(places(laid))
      expect(places(drawn)).toEqual({
        a: [Number(node("alpha").dataset.x), Number(node("alpha").dataset.y)],
        b: [Number(node("beta").dataset.x), Number(node("beta").dataset.y)],
        c: [Number(node("gamma").dataset.x), Number(node("gamma").dataset.y)],
        d: [Number(node("delta").dataset.x), Number(node("delta").dataset.y)],
      })
    } finally {
      vi.useRealTimers()
    }
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

  it("keeps the label of a hovered node and of its neighbours whatever the density", () => {
    renderScreen(<KnowledgeGraph graph={aChain()} layout="force" legend={[]} label="Chain" />)

    expect(node("alpha").dataset.forceLabel).toBe("false")

    fireEvent.mouseEnter(node("beta"))

    expect(node("beta").dataset.forceLabel).toBe("true")
    expect(node("alpha").dataset.forceLabel).toBe("true")
    expect(node("gamma").dataset.forceLabel).toBe("true")
    expect(node("delta").dataset.forceLabel).toBe("false")
  })

  it("names an edge only while the pointer is on one of its ends", () => {
    renderScreen(<KnowledgeGraph graph={aChain()} layout="force" legend={[]} label="Chain" />)

    expect(edge("a-b").dataset.label).toBe("")
    expect(edge("a-b").dataset.forceLabel).toBe("false")

    fireEvent.mouseEnter(node("alpha"))

    expect(edge("a-b").dataset.label).toBe("2")
    expect(edge("a-b").dataset.forceLabel).toBe("true")
    expect(edge("b-c").dataset.label).toBe("")

    fireEvent.mouseLeave(node("alpha"))

    expect(edge("a-b").dataset.label).toBe("")
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
