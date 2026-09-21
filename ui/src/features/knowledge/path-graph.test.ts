/**
 * The Path graph model (022): the hops of a shortest path as a chain, each
 * edge labelled by its kind and dashed where it is a guess.
 */

import { describe, expect, it } from "vitest"

import type { components } from "@/api"

import { pathGraph } from "./path-graph"

type KnowledgePathHopDto = components["schemas"]["KnowledgePathHopDto"]

function hop(name: string, edge?: [kind: string, confidence: string]): KnowledgePathHopDto {
  return {
    repository_id: "01JREPO000000000000000WEB",
    path: `src/${name}.rs`,
    line: 1,
    kind: "function",
    name,
    edge_kind: edge?.[0] ?? null,
    confidence: edge?.[1] ?? null,
  }
}

describe("pathGraph", () => {
  it("chains the hops in order, each edge labelled by its edge kind", () => {
    const { graph } = pathGraph({
      hops: [hop("start"), hop("middle", ["calls", "exact"]), hop("end", ["calls_route", "exact"])],
    })

    expect(graph.mapNodes((_key, attributes) => attributes.label)).toEqual([
      "start",
      "middle",
      "end",
    ])
    expect(
      graph.mapEdges((_key, attributes, _from, _to, from, to) => [
        from.label,
        attributes.label,
        to.label,
      ]),
    ).toEqual([
      ["start", "calls", "middle"],
      ["middle", "calls_route", "end"],
    ])
  })

  it("dashes an edge that is a guess, and no other", () => {
    const { graph } = pathGraph({
      hops: [hop("start"), hop("middle", ["calls", "heuristic"]), hop("end", ["calls", "exact"])],
    })

    expect(graph.mapEdges((_key, attributes) => attributes.dashed)).toEqual([true, false])
  })

  it("tells the two ends of the chain from the hops between them", () => {
    const { graph } = pathGraph({
      hops: [hop("start"), hop("middle", ["calls", "exact"]), hop("end", ["calls", "exact"])],
    })

    expect(graph.mapNodes((_key, attributes) => attributes.tone)).toEqual([
      "active",
      "pending",
      "done",
    ])
  })

  it("names the symbol behind each node", () => {
    const { symbols } = pathGraph({ hops: [hop("start"), hop("end", ["calls", "exact"])] })

    expect([...symbols.values()]).toEqual(["start", "end"])
  })

  it("has no node where there is no path", () => {
    const { graph } = pathGraph({ hops: [] })

    expect(graph.order).toBe(0)
  })
})
