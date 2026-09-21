/**
 * The Impact graph (022): what a change to one symbol reaches, in layers.
 *
 * `GET /v1/knowledge/impact` answers one object for each definition the
 * symbol names. The definition is in the first layer, and each caller in the
 * layer of its `depth`: a direct caller is in the second, a caller of that in
 * the third. Every node is named by its symbol, and clicking one goes to it.
 *
 * The daemon names a caller's depth, not which caller it calls. So an edge
 * is drawn only where the walk makes it certain: from a definition to its
 * direct callers, and from the one node of a layer to the callers of the next.
 * With several nodes in a layer, a caller two steps out sits in its layer with
 * no edge back, rather than an edge to the wrong node. An edge is dashed
 * where the call was a guess.
 */

import type { KnowledgeImpactDto } from "@/api"

import { emptyGraph, type KnowledgeGraphModel } from "./graph/graph-model"

/** The first layer holds the changed definitions; a caller at `depth` is in `depth`. */
const FIRST_LAYER = 0

/** More callers than this and the daemon stops the walk at the definition. */
export const STOP_CALLERS = 200

interface Located {
  repository_id: string
  path: string
  line: number
  name: string
}

/** A definition is its place: the same name in two files is two nodes. */
function definitionKey(definition: Located): string {
  return `${definition.repository_id}:${definition.path}:${definition.line}:${definition.name}`
}

interface ImpactGraph {
  graph: KnowledgeGraphModel
  /** The symbol each node stands for, by its key: the label may say more than the name. */
  symbols: Map<string, string>
}

export function impactGraph(impacts: readonly KnowledgeImpactDto[]): ImpactGraph {
  const graph = emptyGraph()
  const symbols = new Map<string, string>()

  /** A definition another walk already placed keeps the nearer of its two layers. */
  const place = (definition: Located, layer: number, stopped: boolean, changed: boolean) => {
    const key = definitionKey(definition)
    const nearest = graph.hasNode(key) ? graph.getNodeAttribute(key, "layer") : undefined
    graph.mergeNode(key, {
      label: stopped
        ? `${definition.name} (walk stopped: over ${STOP_CALLERS} callers)`
        : definition.name,
      tone: stopped ? "danger" : changed ? "active" : "pending",
      layer: Math.min(layer, nearest ?? layer),
    })
    symbols.set(key, definition.name)
    return key
  }

  for (const impact of impacts) {
    const stopped = new Set(impact.stopped)
    const byDepth = new Map<number, string[]>()
    const add = (depth: number, key: string) =>
      byDepth.set(depth, [...(byDepth.get(depth) ?? []), key])

    add(FIRST_LAYER, place(impact.symbol, FIRST_LAYER, stopped.has(impact.symbol.name), true))
    for (const caller of impact.callers) {
      add(caller.depth, place(caller, caller.depth, stopped.has(caller.name), false))
    }

    for (const caller of impact.callers) {
      const before = byDepth.get(caller.depth - 1)
      const callee = before?.length === 1 ? before[0] : undefined
      const key = definitionKey(caller)
      // An edge that does not lead outwards would only tangle the layers.
      if (callee === undefined || layerOf(graph, callee) >= layerOf(graph, key)) continue
      graph.mergeDirectedEdgeWithKey(`${callee}→${key}`, callee, key, {
        tone: "pending",
        dashed: caller.confidence === "heuristic",
      })
    }
  }

  return { graph, symbols }
}

function layerOf(graph: KnowledgeGraphModel, node: string): number {
  return graph.getNodeAttribute(node, "layer") ?? FIRST_LAYER
}
