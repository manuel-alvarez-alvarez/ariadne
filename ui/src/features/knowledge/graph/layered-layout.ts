/**
 * The layered, left-to-right layout (022): a model in, the same model with
 * every node placed out, for the graphs that read as a chain or as layers —
 * an impact walk and a path — where a force layout would scramble the order.
 *
 * ELK does the layout, in the web worker its own build ships: a large walk
 * never blocks the screen. A node that names a `layer` is put in that layer,
 * and any other goes where its edges lead. Hand the result to `KnowledgeGraph` as `layout="fixed"`.
 *
 * jsdom has no Worker, so a test file that lays a graph out swaps the worker
 * for elkjs's own stand-in that runs on the calling thread
 * (`@/test/elk-worker.ts`).
 */

import ELKConstructor, { type ELK, type ElkExtendedEdge, type ElkNode } from "elkjs/lib/elk-api"
import ElkWorker from "elkjs/lib/elk-worker.min.js?worker"
import { useEffect, useState } from "react"

import type { KnowledgeGraphModel } from "./graph-model"

/** What a node takes up in the layout: its dot and the label beside it. */
const NODE_HEIGHT = 28
const NODE_PADDING = 28
const CHARACTER_WIDTH = 7

let elk: ELK | undefined

/** One worker for the whole app, started by the first layout. */
function layoutEngine(): ELK {
  elk ??= new ELKConstructor({ workerFactory: () => new ElkWorker() })
  return elk
}

function nodeWidth(label: string): number {
  return NODE_PADDING + CHARACTER_WIDTH * label.length
}

/**
 * ELK layers a node by the longest path to it. Where the model names layers,
 * an edge from one node of each layer to the nodes of the next says so: a
 * layout-only edge, drawn by no one.
 */
function layerEdges(graph: KnowledgeGraphModel): ElkExtendedEdge[] {
  const layers = new Map<number, string[]>()
  graph.forEachNode((node, { layer }) => {
    if (layer !== undefined) layers.set(layer, [...(layers.get(layer) ?? []), node])
  })
  const edges: ElkExtendedEdge[] = []
  for (const [layer, nodes] of layers) {
    const [anchor] = layers.get(layer - 1) ?? []
    if (anchor === undefined) continue
    for (const node of nodes) {
      edges.push({ id: `layer:${anchor}→${node}`, sources: [anchor], targets: [node] })
    }
  }
  return edges
}

export async function layeredLayout(graph: KnowledgeGraphModel): Promise<KnowledgeGraphModel> {
  const root: ElkNode = {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.spacing.nodeNode": "24",
      "elk.layered.spacing.nodeNodeBetweenLayers": "80",
    },
    children: graph.mapNodes((node, attributes) => ({
      id: node,
      width: nodeWidth(attributes.label),
      height: NODE_HEIGHT,
    })),
    edges: [
      ...graph.mapEdges((edge, _attributes, source, target) => ({
        id: edge,
        sources: [source],
        targets: [target],
      })),
      ...layerEdges(graph),
    ],
  }

  const laid = await layoutEngine().layout(root)

  const positioned = graph.copy()
  for (const child of laid.children ?? []) {
    // ELK gives the top-left corner of the box; the graph wants the node's centre.
    positioned.mergeNodeAttributes(child.id, {
      x: (child.x ?? 0) + (child.width ?? 0) / 2,
      y: (child.y ?? 0) + (child.height ?? 0) / 2,
    })
  }
  return positioned
}

/**
 * The model laid out, once ELK answers. It is `undefined` while the layout
 * runs, and after it fails: a graph the layout never saw is not drawn, since
 * every node would sit on one point.
 */
export function useLayeredLayout(graph: KnowledgeGraphModel | undefined): {
  graph: KnowledgeGraphModel | undefined
  error: Error | undefined
} {
  const [done, setDone] = useState<{
    source: KnowledgeGraphModel
    graph?: KnowledgeGraphModel
    error?: Error
  }>()

  useEffect(() => {
    if (!graph) return
    let current = true
    layeredLayout(graph).then(
      (positioned) => current && setDone({ source: graph, graph: positioned }),
      (error: unknown) =>
        current &&
        setDone({
          source: graph,
          error: error instanceof Error ? error : new Error(String(error)),
        }),
    )
    return () => {
      current = false
    }
  }, [graph])

  // A layout of an earlier model is not this one's.
  const answered = done?.source === graph ? done : undefined
  return { graph: answered?.graph, error: answered?.error }
}
