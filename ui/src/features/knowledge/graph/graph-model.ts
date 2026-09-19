/**
 * What a knowledge graph is made of, before anything draws it: a graphology
 * graph whose nodes and edges name a tone rather than a colour, and the
 * rules that turn a hover into what is highlighted.
 *
 * A tone is a step of the status ramp in `index.css`. The model names it, and
 * `KnowledgeGraph` resolves it against the theme on screen, so a model built
 * once reads right in both themes, and a test reads the model with no
 * stylesheet at all.
 */

import Graph from "graphology"

/** A step of the status ramp: `--status-<tone>` in `index.css`. */
export type GraphTone =
  | "pending"
  | "ready"
  | "active"
  | "review"
  | "approved"
  | "done"
  | "warn"
  | "danger"

export interface GraphNodeAttributes {
  label: string
  tone: GraphTone
  /** The radius on screen, in pixels. */
  size?: number
  /** Where the layout put it; the component places a node that has none. */
  x?: number
  y?: number
}

export interface GraphEdgeAttributes {
  /** Drawn at the middle of the edge. */
  label?: string
  tone: GraphTone
  /** A dashed line: what the edge rests on is a guess. */
  dashed?: boolean
  /** The width on screen, in pixels. */
  size?: number
}

export type KnowledgeGraphModel = Graph<GraphNodeAttributes, GraphEdgeAttributes>

/** How the component places the nodes: a force layout, or a ring. */
export type GraphLayout = "force" | "circle"

/** One row of the legend under a graph. */
export interface LegendEntry {
  label: string
  tone: GraphTone
  /** Shown as a dashed line rather than a swatch. */
  dashed?: boolean
}

/** A directed graph that holds several edges between one pair: one per kind. */
export function emptyGraph(): KnowledgeGraphModel {
  return new Graph<GraphNodeAttributes, GraphEdgeAttributes>({ multi: true, type: "directed" })
}

/** The node and each node one edge away from it, either way. */
function neighbourhood(graph: KnowledgeGraphModel, node: string): Set<string> {
  return new Set([node, ...graph.neighbors(node)])
}

/**
 * A copy of the graph with every node placed: a node the model already
 * placed keeps its place, and the rest go round a ring. A force layout
 * starts from the ring too, as it needs somewhere to start from.
 */
export function placed(graph: KnowledgeGraphModel): KnowledgeGraphModel {
  const copy = graph.copy()
  const count = copy.order
  let next = 0
  copy.forEachNode((node, attributes) => {
    const index = next++
    if (attributes.x !== undefined && attributes.y !== undefined) return
    const angle = (2 * Math.PI * index) / Math.max(count, 1)
    copy.mergeNodeAttributes(node, { x: Math.cos(angle), y: Math.sin(angle) })
  })
  return copy
}

/** What the renderer is told about one node or edge while a node is hovered. */
interface Emphasis {
  /** Drawn in its own tone, with its label. */
  highlighted: boolean
  /** Drawn in the faded colour, without a label. */
  faded: boolean
}

/**
 * The emphasis of each node and edge while `hovered` is under the pointer:
 * the node and its neighbours stand out, and the rest fade. With nothing
 * hovered, nothing is either.
 */
export function emphasis(graph: KnowledgeGraphModel, hovered: string | null) {
  const near = hovered !== null && graph.hasNode(hovered) ? neighbourhood(graph, hovered) : null
  return {
    node(node: string): Emphasis {
      if (!near) return { highlighted: false, faded: false }
      return { highlighted: near.has(node), faded: !near.has(node) }
    },
    edge(edge: string): Emphasis {
      if (!near || hovered === null) return { highlighted: false, faded: false }
      const touches = graph.hasExtremity(edge, hovered)
      return { highlighted: touches, faded: !touches }
    },
  }
}
