/**
 * The Files tab's graph (022): the files of one repository ref and the
 * edges between them, from `GET /v1/knowledge/graph`.
 *
 * The model is built once per response and level. At file level a node is a
 * file; at directory level the files of one directory, cut to `depth` path
 * segments, are one node, and their edges are summed. A node is sized by its
 * symbols and coloured by its top-level directory; an edge carries its count
 * as the weight the force layout pulls with.
 *
 * The filters never rebuild the model: {@link filesVisibility} names what to
 * hide, and the graph component hides it.
 */

import type { KnowledgeGraphDto } from "@/api"

import {
  emptyGraph,
  type GraphTone,
  type GraphVisibility,
  type KnowledgeGraphModel,
  type LegendEntry,
} from "./graph/graph-model"

export type FilesLevel = "file" | "directory"

/** What a file at the root of the repository counts as, for its colour and its directory. */
const ROOT_DIRECTORY = "(root)"

/** The tones the directories are coloured in, largest directory first. */
const DIRECTORY_TONES: readonly GraphTone[] = [
  "active",
  "review",
  "approved",
  "warn",
  "ready",
  "danger",
  "done",
]
/** The tone every directory past {@link DIRECTORY_TONES} shares. */
const OTHER_TONE: GraphTone = "pending"

/** One graph edge: the grouped file edges under it, summed. */
interface FilesEdge {
  from: string
  to: string
  kind: string
  count: number
  confidence: "exact" | "heuristic"
}

interface FilesGraph {
  graph: KnowledgeGraphModel
  /** A swatch per coloured top-level directory, and one for the rest. */
  legend: LegendEntry[]
  /** Every edge kind in the response, sorted. */
  kinds: string[]
  /** By the key of the graph edge. */
  edges: Map<string, FilesEdge>
}

export interface FilesFilters {
  /** A node is shown when its path holds this text, in any case. Empty shows all. */
  text: string
  /** The edge kinds turned off. */
  hiddenKinds: ReadonlySet<string>
  /** Hide a node with no shown edge. */
  hideIsolated: boolean
}

function topDirectory(path: string): string {
  const slash = path.indexOf("/")
  return slash < 0 ? ROOT_DIRECTORY : path.slice(0, slash)
}

/** The directory of a file, cut to its first `depth` segments. */
function directoryOf(path: string, depth: number): string {
  const segments = path.split("/").slice(0, -1)
  if (segments.length === 0) return ROOT_DIRECTORY
  return segments.slice(0, Math.max(1, depth)).join("/")
}

function lastSegment(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1)
}

/** Bigger with more symbols, slowly: a file of four hundred is not a hundred times one of four. */
function nodeSize(symbols: number): number {
  return 3 + Math.min(15, Math.sqrt(symbols))
}

/** The top-level directories by how many files each holds, with the tone each is drawn in. */
function directoryTones(dto: KnowledgeGraphDto): {
  tones: Map<string, GraphTone>
  legend: LegendEntry[]
} {
  const files = new Map<string, number>()
  for (const node of dto.nodes) {
    const top = topDirectory(node.path)
    files.set(top, (files.get(top) ?? 0) + 1)
  }
  const ranked = [...files.entries()]
    .sort(([a, left], [b, right]) => right - left || a.localeCompare(b))
    .map(([name]) => name)

  const tones = new Map<string, GraphTone>()
  const legend: LegendEntry[] = []
  ranked.forEach((name, index) => {
    const tone = DIRECTORY_TONES[index] ?? OTHER_TONE
    tones.set(name, tone)
    if (index < DIRECTORY_TONES.length) legend.push({ label: name, tone })
  })
  if (ranked.length > DIRECTORY_TONES.length) legend.push({ label: "Other", tone: OTHER_TONE })
  return { tones, legend }
}

export function filesGraph(dto: KnowledgeGraphDto, level: FilesLevel, depth: number): FilesGraph {
  const { tones, legend } = directoryTones(dto)
  const graph = emptyGraph()
  const nodeOf = (path: string) => (level === "file" ? path : directoryOf(path, depth))

  // Every file of one node shares its top-level directory, and so its tone.
  const symbols = new Map<string, { count: number; tone: GraphTone }>()
  for (const node of dto.nodes) {
    const key = nodeOf(node.path)
    const tone = tones.get(topDirectory(node.path)) ?? OTHER_TONE
    symbols.set(key, { count: (symbols.get(key)?.count ?? 0) + node.symbols, tone })
  }
  for (const [key, { count, tone }] of symbols) {
    graph.addNode(key, {
      label: level === "file" ? lastSegment(key) : key,
      tone,
      size: nodeSize(count),
    })
  }

  const edges = new Map<string, FilesEdge>()
  for (const edge of dto.edges) {
    const from = nodeOf(edge.from)
    const to = nodeOf(edge.to)
    // The edges inside one directory say nothing about the directories.
    if (from === to || !graph.hasNode(from) || !graph.hasNode(to)) continue
    // A path may hold any character: a JSON array keeps the three apart.
    const key = JSON.stringify([from, to, edge.kind])
    const summed = edges.get(key)
    if (summed) {
      summed.count += edge.count
      if (edge.confidence === "heuristic") summed.confidence = "heuristic"
    } else {
      edges.set(key, { from, to, kind: edge.kind, count: edge.count, confidence: edge.confidence })
    }
  }
  for (const [key, edge] of edges) {
    graph.addDirectedEdgeWithKey(key, edge.from, edge.to, {
      tone: "pending",
      dashed: edge.confidence === "heuristic",
      size: 1 + Math.log2(edge.count),
      weight: edge.count,
    })
  }

  const kinds = [...new Set(dto.edges.map((edge) => edge.kind))].sort()
  return { graph, legend, kinds, edges }
}

/** What the filters hide, as keys of the model: the model itself is left as it was built. */
export function filesVisibility(built: FilesGraph, filters: FilesFilters): GraphVisibility {
  const { graph, edges } = built
  const text = filters.text.trim().toLowerCase()
  const nodes = new Set<string>()
  if (text) {
    graph.forEachNode((node) => {
      if (!node.toLowerCase().includes(text)) nodes.add(node)
    })
  }

  const hiddenEdges = new Set<string>()
  const linked = new Set<string>()
  for (const [key, edge] of edges) {
    if (filters.hiddenKinds.has(edge.kind) || nodes.has(edge.from) || nodes.has(edge.to)) {
      hiddenEdges.add(key)
    } else {
      linked.add(edge.from)
      linked.add(edge.to)
    }
  }

  if (filters.hideIsolated) {
    graph.forEachNode((node) => {
      if (!linked.has(node)) nodes.add(node)
    })
  }
  return { nodes, edges: hiddenEdges }
}
