/**
 * The force layout (022): the damped pull of an edge, and the places the
 * layout hands back before the first frame.
 *
 * It needs no browser: it is a function from a graph to the same graph with
 * every node placed, which is the point — the view draws what it is given
 * and moves nothing afterwards.
 */

import { describe, expect, it, vi } from "vitest"

import { forceLayout } from "./force-layout"
import { emptyGraph, type KnowledgeGraphModel, NODE_SIZE } from "./graph-model"

/** Two groups of five, each joined inside, with one edge between them. */
function twoClusters(): KnowledgeGraphModel {
  const graph = emptyGraph()
  for (const side of ["a", "b"]) {
    for (let index = 0; index < 5; index++) {
      graph.addNode(`${side}${index}`, { label: `${side}${index}`, tone: "pending" })
    }
    for (let from = 0; from < 5; from++) {
      for (let to = from + 1; to < 5; to++) {
        graph.addDirectedEdgeWithKey(`${side}${from}-${to}`, `${side}${from}`, `${side}${to}`, {
          tone: "active",
        })
      }
    }
  }
  graph.addDirectedEdgeWithKey("bridge", "a0", "b0", { tone: "active" })
  return graph
}

/** Every node's place, by key. */
function places(graph: KnowledgeGraphModel): Record<string, [number, number]> {
  const out: Record<string, [number, number]> = {}
  graph.forEachNode((node, attributes) => {
    out[node] = [attributes.x ?? Number.NaN, attributes.y ?? Number.NaN]
  })
  return out
}

/** How far apart two nodes ended. */
function apart(graph: KnowledgeGraphModel, one: string, other: string): number {
  const a = graph.getNodeAttributes(one)
  const b = graph.getNodeAttributes(other)
  return Math.hypot((a.x ?? 0) - (b.x ?? 0), (a.y ?? 0) - (b.y ?? 0))
}

/**
 * The narrowest span sigma draws a force graph in, in pixels.
 *
 * It is the window at the width the side pane opens beside the graph, `lg`,
 * 1024 pixels: the sidebar takes 224, the padding of the main area takes 48,
 * the pane takes 384, its gap takes 12, the card's border takes 2, and
 * sigma's `stagePadding` takes 30 from each side. A narrower window stacks
 * the pane under the graph and hands the graph the whole row, and a wider one
 * hands it every pixel it gains, so no supported window is tighter than this.
 *
 * The width is the shorter side there: the card is 354 pixels across, and
 * the shortest card is 448 pixels high with 24 for its legend, which leaves
 * 424. sigma takes 30 from each side of the shorter one.
 */
const VIEW_SPAN_PX = 1024 - 224 - 48 - 384 - 12 - 2 - 2 * 30

/**
 * The span of the same card on a desktop 1440 pixels wide.
 *
 * There the card is 770 pixels across and the height binds instead: the card
 * is 448 pixels high, its legend takes 24, and sigma takes 30 from the top
 * and 30 from the bottom of what is left.
 */
const WIDE_VIEW_SPAN_PX = 448 - 24 - 2 * 30

/**
 * The graph as the view draws it: sigma fits what it is given into its box
 * and draws a node the same size wherever the camera stands, so a place is
 * worth less the wider the graph is and a radius is worth the same always.
 */
function drawn(graph: KnowledgeGraphModel, box = VIEW_SPAN_PX): KnowledgeGraphModel {
  const copy = graph.copy()
  const scale = box / across(graph)
  copy.forEachNode((node, attributes) => {
    copy.mergeNodeAttributes(node, {
      x: (attributes.x ?? 0) * scale,
      y: (attributes.y ?? 0) * scale,
    })
  })
  return copy
}

/** How wide the graph is, in its own places: its longer side. */
function across(graph: KnowledgeGraphModel): number {
  let minX = Number.POSITIVE_INFINITY
  let maxX = Number.NEGATIVE_INFINITY
  let minY = Number.POSITIVE_INFINITY
  let maxY = Number.NEGATIVE_INFINITY
  graph.forEachNode((_node, attributes) => {
    minX = Math.min(minX, attributes.x ?? 0)
    maxX = Math.max(maxX, attributes.x ?? 0)
    minY = Math.min(minY, attributes.y ?? 0)
    maxY = Math.max(maxY, attributes.y ?? 0)
  })
  return Math.max(maxX - minX, maxY - minY, 1)
}

/**
 * The clear space between the closest pair of discs, in pixels: what is left
 * between them once each radius is taken off. Below zero, one node is drawn
 * over another.
 */
function closest(graph: KnowledgeGraphModel): number {
  const discs = graph.mapNodes((_node, attributes) => ({
    x: attributes.x ?? 0,
    y: attributes.y ?? 0,
    radius: attributes.size ?? NODE_SIZE,
  }))
  let clear = Number.POSITIVE_INFINITY
  for (let one = 0; one < discs.length; one++) {
    for (let other = one + 1; other < discs.length; other++) {
      const a = discs[one]
      const b = discs[other]
      if (!a || !b) continue
      clear = Math.min(clear, Math.hypot(a.x - b.x, a.y - b.y) - a.radius - b.radius)
    }
  }
  return clear
}

/**
 * A graph of `count` files in four directories, sized as the Files tab sizes
 * them: joined up inside a directory, and now and then across one.
 */
function filesGraph(count: number): KnowledgeGraphModel {
  const graph = emptyGraph()
  const directories = 4
  const each = Math.ceil(count / directories)
  for (let index = 0; index < count; index++) {
    graph.addNode(`f${index}`, {
      label: `file-${index}`,
      tone: "pending",
      // `nodeSize` in `files-graph.ts`: 3 + the root of its symbol count.
      size: 3 + Math.min(15, Math.sqrt((index * 37) % 260)),
    })
  }
  for (let index = 0; index < count; index++) {
    const directory = Math.floor(index / each)
    for (let step = 1; step <= 4; step++) {
      const other =
        step === 4
          ? (index * 7 + 13) % count
          : Math.min(count - 1, directory * each + ((index + step) % each))
      if (other === index) continue
      const key = `e${index}-${step}`
      if (graph.hasEdge(key)) continue
      graph.addDirectedEdgeWithKey(key, `f${index}`, `f${other}`, {
        tone: "active",
        weight: ((index * step) % 400) + 1,
      })
    }
  }
  return graph
}

describe("the force layout", () => {
  it("gives every node a place, and the same place on a second run", () => {
    const graph = twoClusters()

    const once = forceLayout(graph)
    const twice = forceLayout(graph)

    for (const [node, [x, y]] of Object.entries(places(once))) {
      expect(Number.isFinite(x), `x of ${node}`).toBe(true)
      expect(Number.isFinite(y), `y of ${node}`).toBe(true)
    }
    expect(places(twice)).toEqual(places(once))
    // The caller's model is left as it was built.
    expect(graph.getNodeAttribute("a0", "x")).toBeUndefined()
  })

  it("holds what the edges join together, and pushes the rest away", () => {
    const laid = forceLayout(twoClusters())

    expect(apart(laid, "a1", "a2")).toBeLessThan(apart(laid, "a1", "b1"))
    expect(apart(laid, "b1", "b2")).toBeLessThan(apart(laid, "b1", "a2"))
  })

  it("spreads nodes the model put in one place, rather than drawing them on each other", () => {
    const graph = twoClusters()
    // Every node on the origin: the layout divides by the distance between
    // two nodes, and there is none here.
    graph.forEachNode((node) => graph.mergeNodeAttributes(node, { x: 0, y: 0 }))

    const laid = forceLayout(graph)

    for (const [node, [x, y]] of Object.entries(places(laid))) {
      expect(Number.isFinite(x), `x of ${node}`).toBe(true)
      expect(Number.isFinite(y), `y of ${node}`).toBe(true)
    }
    expect(closest(laid)).toBeGreaterThanOrEqual(0)
    expect(apart(laid, "a1", "b1")).toBeGreaterThan(apart(laid, "a1", "a2"))
  })

  it("draws no node on top of another, on the narrowest desktop it is drawn on", () => {
    // Ten nodes of the size a graph gives a node that names none: no pair may
    // be drawn closer than the two radii, twenty pixels.
    expect(closest(drawn(forceLayout(twoClusters())))).toBeGreaterThanOrEqual(0)

    // And a hundred and sixty files, each sized by the symbols it holds.
    const files = drawn(forceLayout(filesGraph(160)))

    expect(closest(files)).toBeGreaterThanOrEqual(0)
  })

  it("draws smaller discs where the card holds less disc than the graph has", () => {
    // The Files tab of a repository this size: five hundred and ninety-two
    // files, whose discs at their own sizes cover several cards. No
    // arrangement of discs keeps them off each other in a box they cover, so
    // the layout draws every disc smaller instead of drawing one over another.
    const crowded = filesGraph(592)

    const laid = forceLayout(crowded)

    // Clear on the narrowest desktop the app draws a graph on, and clearer
    // on a wider one: the places grow with the canvas and the discs do not.
    expect(closest(drawn(laid))).toBeGreaterThanOrEqual(0)
    expect(closest(drawn(laid, WIDE_VIEW_SPAN_PX))).toBeGreaterThan(closest(drawn(laid)))
    // One share comes off every radius, down to the two pixels that keep a
    // node visible, so the file with the most symbols is still the biggest
    // node on the screen and every node is still drawn.
    const drawnSizes = crowded.mapNodes((node) => ({
      was: crowded.getNodeAttribute(node, "size") ?? NODE_SIZE,
      now: laid.getNodeAttribute(node, "size") ?? NODE_SIZE,
    }))
    for (const size of drawnSizes) {
      expect(size.now).toBeGreaterThanOrEqual(2)
      expect(size.now).toBeLessThan(size.was)
    }
    const biggest = drawnSizes.reduce((a, b) => (a.was > b.was ? a : b))
    const smallest = drawnSizes.reduce((a, b) => (a.was < b.was ? a : b))
    expect(biggest.now).toBeGreaterThan(smallest.now)
    // Placing six hundred nodes takes a second and a half of this runner's
    // own time, and several under the load of a whole suite beside it.
  }, 60_000)

  it("keeps the shape while it pushes the discs apart", () => {
    const laid = forceLayout(filesGraph(160))

    // A file is nearer the files of its own directory than the far side of
    // the graph, which the pushing apart must not undo.
    expect(apart(laid, "f1", "f2")).toBeLessThan(apart(laid, "f1", "f120"))
  })

  it("says the last word on where a node is: nothing is left running to move one", () => {
    vi.useFakeTimers()
    try {
      const laid = forceLayout(twoClusters())
      const settled = places(laid)

      // The layout is a function, not a worker on a timer: it leaves none.
      expect(vi.getTimerCount()).toBe(0)
      vi.advanceTimersByTime(30_000)

      expect(places(laid)).toEqual(settled)
    } finally {
      vi.useRealTimers()
    }
  })

  it("places the one node of a graph nothing can push", () => {
    const graph = emptyGraph()
    graph.addNode("only", { label: "only", tone: "pending" })

    const laid = forceLayout(graph)

    expect(Number.isFinite(laid.getNodeAttribute("only", "x"))).toBe(true)
    expect(Number.isFinite(laid.getNodeAttribute("only", "y"))).toBe(true)
  })
})
