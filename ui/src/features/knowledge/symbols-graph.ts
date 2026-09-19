/** Build the Symbols tab's neighbourhood before the shared graph draws it. */

import type { KnowledgeRelatedDto, KnowledgeSymbolDto } from "@/api"

import { emptyGraph, type GraphTone, type KnowledgeGraphModel } from "./graph/graph-model"

export const SYMBOL_RELATIONS = [
  "callers",
  "callees",
  "implementations",
  "references",
  "tests",
] as const

type SymbolRelation = (typeof SYMBOL_RELATIONS)[number]

export const SYMBOL_RELATION_META: Record<SymbolRelation, { label: string; tone: GraphTone }> = {
  callers: { label: "Callers", tone: "active" },
  callees: { label: "Callees", tone: "ready" },
  implementations: { label: "Implementations", tone: "review" },
  references: { label: "References", tone: "warn" },
  tests: { label: "Tests", tone: "danger" },
}

export interface SymbolTarget {
  name: string
  repositoryId: string
  path: string
  line: number
}

interface SymbolsGraphModel {
  graph: KnowledgeGraphModel
  /** Clickable graph nodes, by their graph key. Aggregate nodes have no target. */
  targets: Map<string, SymbolTarget>
}

function nodeKey(relation: SymbolRelation, related: KnowledgeRelatedDto, index: number): string {
  return [relation, related.repository_id, related.path, related.line, related.name, index].join(
    ":",
  )
}

function position(relationIndex: number, itemIndex: number, itemCount: number) {
  const sector = (2 * Math.PI * relationIndex) / SYMBOL_RELATIONS.length - Math.PI / 2
  const spread = Math.min(Math.PI / 3, itemCount * 0.18)
  const offset = itemCount === 1 ? 0 : (itemIndex / (itemCount - 1) - 0.5) * spread
  const radius = 1.5 + (itemIndex % 2) * 0.25
  return { x: Math.cos(sector + offset) * radius, y: Math.sin(sector + offset) * radius }
}

/** Put one definition at the centre and each context relation in its own sector. */
export function symbolsGraph(symbol: KnowledgeSymbolDto): SymbolsGraphModel {
  const graph = emptyGraph()
  const targets = new Map<string, SymbolTarget>()
  graph.addNode("centre", { label: symbol.name, tone: "done", size: 15, x: 0, y: 0 })

  for (const [relationIndex, relation] of SYMBOL_RELATIONS.entries()) {
    const meta = SYMBOL_RELATION_META[relation]
    const related = symbol.context?.[relation] ?? []
    const more = symbol.context?.more?.[relation] ?? 0
    const itemCount = related.length + (more > 0 ? 1 : 0)

    related.forEach((item, itemIndex) => {
      const key = nodeKey(relation, item, itemIndex)
      const external = item.repository_id !== symbol.repository_id
      graph.addNode(key, {
        label: `${item.name}${external ? " ↗" : ""}`,
        tone: meta.tone,
        ...position(relationIndex, itemIndex, itemCount),
      })
      const [from, to] = relation === "callees" ? ["centre", key] : [key, "centre"]
      graph.addDirectedEdgeWithKey(`${key}→${relation}`, from, to, {
        label: relation,
        tone: meta.tone,
        dashed: item.confidence === "heuristic",
      })
      targets.set(key, {
        name: item.name,
        repositoryId: item.repository_id,
        path: item.path,
        line: item.line,
      })
    })

    if (more > 0) {
      const key = `more:${relation}`
      graph.addNode(key, {
        label: `+${more} more`,
        tone: meta.tone,
        size: 8,
        ...position(relationIndex, related.length, itemCount),
      })
      const [from, to] = relation === "callees" ? ["centre", key] : [key, "centre"]
      graph.addDirectedEdgeWithKey(`${key}→${relation}`, from, to, {
        label: relation,
        tone: meta.tone,
      })
    }
  }

  return { graph, targets }
}
