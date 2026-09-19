/**
 * The Repositories tab's graph model (022): a node per repository, named by
 * its folder, and one edge per pair and kind, counted, coloured by kind, and
 * dashed where every edge under it is heuristic.
 */

import { describe, expect, it } from "vitest"

import type { KnowledgeEdgeDto, KnowledgeInteractionGroupDto } from "@/api"
import { aRepository } from "@/test/fixtures"

import { type InteractionFilters, repositoriesGraph } from "./repositories-graph"

const WEB = aRepository({ id: "01JREPO000000000000000WEB", path: "/home/me/dev/web" })
const API = aRepository({ id: "01JREPO000000000000000API", path: "/home/me/dev/api" })
const REPOSITORIES = [WEB, API]

const EVERYTHING: InteractionFilters = {
  kinds: new Set(["depends_on", "references", "calls_route", "sets_env"]),
  confidence: "all",
}

function anEdge(line: number, confidence: "exact" | "heuristic" = "exact"): KnowledgeEdgeDto {
  return {
    from: { repository_id: WEB.id, path: "src/client.ts", line, symbol: "fetchGoal" },
    to: { repository_id: API.id, path: "src/routes.rs", line: 88, symbol: "get_goal" },
    confidence,
    step: "route",
    candidates: 1,
  }
}

function group(kind: string, edges: KnowledgeEdgeDto[]): KnowledgeInteractionGroupDto {
  return { kind, edges }
}

describe("repositoriesGraph", () => {
  it("has one node per repository, named by its folder and never by its id", () => {
    const { graph } = repositoriesGraph(REPOSITORIES, [], EVERYTHING)

    expect(graph.nodes()).toEqual([WEB.id, API.id])
    expect(graph.getNodeAttribute(WEB.id, "label")).toBe("web")
    expect(graph.getNodeAttribute(API.id, "label")).toBe("api")
  })

  it("groups the edges of one pair and one kind into one edge, with their count", () => {
    const { graph, groups } = repositoriesGraph(
      REPOSITORIES,
      [[group("calls_route", [anEdge(1), anEdge(2), anEdge(3)])]],
      EVERYTHING,
    )

    expect(graph.size).toBe(1)
    const [key] = graph.edges()
    expect(graph.source(key)).toBe(WEB.id)
    expect(graph.target(key)).toBe(API.id)
    expect(graph.getEdgeAttribute(key, "label")).toBe("3")
    expect(groups.get(key ?? "")?.edges.map((edge) => edge.from.line)).toEqual([1, 2, 3])
  })

  it("keeps one edge per kind between the same pair, each in its kind's colour", () => {
    const { graph } = repositoriesGraph(
      REPOSITORIES,
      [[group("calls_route", [anEdge(1)]), group("depends_on", [anEdge(2)])]],
      EVERYTHING,
    )

    const tones = graph.mapEdges((_key, attributes) => attributes.tone)
    expect(tones).toHaveLength(2)
    expect(new Set(tones).size).toBe(2)
  })

  it("counts an edge once when both of its repositories answer with it", () => {
    const answer = [group("calls_route", [anEdge(1)])]

    const { graph } = repositoriesGraph(REPOSITORIES, [answer, answer], EVERYTHING)

    expect(graph.getEdgeAttribute(graph.edges()[0] ?? "", "label")).toBe("1")
  })

  it("dashes an edge whose edges are all heuristic, and only that one", () => {
    const guessed = repositoriesGraph(
      REPOSITORIES,
      [[group("calls_route", [anEdge(1, "heuristic"), anEdge(2, "heuristic")])]],
      EVERYTHING,
    ).graph
    const mixed = repositoriesGraph(
      REPOSITORIES,
      [[group("calls_route", [anEdge(1, "heuristic"), anEdge(2, "exact")])]],
      EVERYTHING,
    ).graph

    expect(guessed.getEdgeAttribute(guessed.edges()[0] ?? "", "dashed")).toBe(true)
    expect(mixed.getEdgeAttribute(mixed.edges()[0] ?? "", "dashed")).toBe(false)
  })

  it("leaves out the kinds and the confidence the filters turn off", () => {
    const answers = [
      [
        group("calls_route", [anEdge(1, "heuristic"), anEdge(2, "exact")]),
        group("depends_on", [anEdge(3)]),
      ],
    ]

    const exactRoutes = repositoriesGraph(REPOSITORIES, answers, {
      kinds: new Set(["calls_route"]),
      confidence: "exact",
    }).graph

    expect(exactRoutes.size).toBe(1)
    expect(exactRoutes.getEdgeAttribute(exactRoutes.edges()[0] ?? "", "label")).toBe("1")
  })

  it("leaves out an edge to a repository that is not registered", () => {
    const stranger = anEdge(1)
    stranger.to = { ...stranger.to, repository_id: "01JREPO0000000000000GONE" }

    const { graph } = repositoriesGraph(
      REPOSITORIES,
      [[group("calls_route", [stranger])]],
      EVERYTHING,
    )

    expect(graph.size).toBe(0)
    expect(graph.order).toBe(2)
  })
})
