/**
 * The Path graph (022): the shortest directed path between two symbols, as a
 * chain from the first to the last.
 *
 * `GET /v1/knowledge/path` answers the hops in order, each with the kind and
 * the confidence of the edge into it — none on the first. A hop is a node,
 * keyed by its place in the chain, and an edge is labelled by its kind and
 * dashed where the edge was a guess. Each node is named by its symbol, and
 * clicking one goes to it.
 */

import type { KnowledgePathDto } from "@/api"

import { emptyGraph, type KnowledgeGraphModel } from "./graph/graph-model"

interface PathGraph {
  graph: KnowledgeGraphModel
  /** The symbol each node stands for, by its key. */
  symbols: Map<string, string>
}

export function pathGraph({ hops }: KnowledgePathDto): PathGraph {
  const graph = emptyGraph()
  const symbols = new Map<string, string>()

  hops.forEach((hop, index) => {
    const key = String(index)
    graph.addNode(key, {
      label: hop.name,
      tone: index === 0 ? "active" : index === hops.length - 1 ? "done" : "pending",
    })
    symbols.set(key, hop.name)
    if (index === 0) return
    graph.addDirectedEdgeWithKey(`${index - 1}→${key}`, String(index - 1), key, {
      label: hop.edge_kind ?? "",
      tone: "ready",
      dashed: hop.confidence === "heuristic",
    })
  })

  return { graph, symbols }
}
