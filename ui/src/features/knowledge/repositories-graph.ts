/**
 * The Repositories tab's graph (022): one node per registered repository,
 * and one edge per pair and kind of interaction between them.
 *
 * `GET /v1/knowledge/interactions` answers for one repository at a time, with
 * its edges both ways, so the tab asks once per repository and an edge
 * between two of them comes back twice: it is counted once. The edges of one
 * pair and one kind become one graph edge, with their count as its label and
 * the kind's colour; it is dashed where every edge under it is a guess.
 */

import type { KnowledgeEdgeDto, KnowledgeInteractionGroupDto, RepositoryDto } from "@/api"
import { folderName } from "@/lib/format"

import { emptyGraph, type KnowledgeGraphModel } from "./graph/graph-model"
import { INTERACTION_KIND_TONES } from "./status"
import type { KnowledgeConfidence, KnowledgeInteractionKind } from "./types"

export interface InteractionFilters {
  /** The kinds shown; an edge of any other kind is left out. */
  kinds: ReadonlySet<string>
  /** One confidence, or both. */
  confidence: KnowledgeConfidence | "all"
}

/** The file-level edges under one graph edge: what a click on it lists. */
interface GroupedInteraction {
  kind: string
  from: string
  to: string
  edges: KnowledgeEdgeDto[]
}

interface RepositoriesGraph {
  graph: KnowledgeGraphModel
  /** By the key of the graph edge. */
  groups: Map<string, GroupedInteraction>
}

/** Each distinct edge of every answer once, with its kind. */
function distinctInteractions(
  answers: readonly KnowledgeInteractionGroupDto[][],
): { kind: string; edge: KnowledgeEdgeDto }[] {
  const seen = new Set<string>()
  const found: { kind: string; edge: KnowledgeEdgeDto }[] = []
  for (const groups of answers) {
    for (const group of groups) {
      for (const edge of group.edges) {
        const key = JSON.stringify([group.kind, edge.from, edge.to, edge.confidence, edge.step])
        if (seen.has(key)) continue
        seen.add(key)
        found.push({ kind: group.kind, edge })
      }
    }
  }
  return found
}

export function repositoriesGraph(
  repositories: readonly RepositoryDto[],
  answers: readonly KnowledgeInteractionGroupDto[][],
  filters: InteractionFilters,
): RepositoriesGraph {
  const graph = emptyGraph()
  for (const repository of repositories) {
    graph.addNode(repository.id, { label: folderName(repository.path), tone: "pending" })
  }

  const groups = new Map<string, GroupedInteraction>()
  for (const { kind, edge } of distinctInteractions(answers)) {
    if (!filters.kinds.has(kind)) continue
    if (filters.confidence !== "all" && edge.confidence !== filters.confidence) continue
    const from = edge.from.repository_id
    const to = edge.to.repository_id
    // An end no registered repository answers to has no name to show.
    if (!graph.hasNode(from) || !graph.hasNode(to)) continue
    const key = `${from}→${to}:${kind}`
    const group = groups.get(key) ?? { kind, from, to, edges: [] }
    group.edges.push(edge)
    groups.set(key, group)
  }

  for (const [key, group] of groups) {
    const count = group.edges.length
    graph.addDirectedEdgeWithKey(key, group.from, group.to, {
      label: String(count),
      tone: INTERACTION_KIND_TONES[group.kind as KnowledgeInteractionKind] ?? "pending",
      dashed: group.edges.every((edge) => edge.confidence === "heuristic"),
      // Wider as the count grows, slowly: a hundred edges is not fifty times one.
      size: 1.5 + Math.log2(count),
    })
  }

  return { graph, groups }
}
