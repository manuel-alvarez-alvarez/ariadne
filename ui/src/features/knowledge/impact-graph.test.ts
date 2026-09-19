/**
 * The Impact graph model (022): the changed definition in the first layer,
 * each caller in the layer of its depth, a dashed edge where a call is a
 * guess, and a node that says so where the walk stopped.
 */

import { describe, expect, it } from "vitest"

import type { KnowledgeImpactCallerDto, KnowledgeImpactDto } from "@/api"

import { definitionKey, impactGraph } from "./impact-graph"

const REPOSITORY = "01JREPO000000000000000WEB"

function definition(name: string, line: number) {
  return { repository_id: REPOSITORY, path: `src/${name}.rs`, line, name }
}

function caller(
  name: string,
  depth: number,
  confidence: "exact" | "heuristic" = "exact",
): KnowledgeImpactCallerDto {
  return { ...definition(name, 1), depth, confidence, step: "file", candidates: 1 }
}

function anImpact(callers: KnowledgeImpactCallerDto[], stopped: string[] = []): KnowledgeImpactDto {
  return {
    symbol: {
      ...definition("parse", 10),
      confidence: "exact",
      step: null,
      candidates: 1,
    },
    callers,
    stopped,
  }
}

/** Where each node of the model sits, by symbol name. */
function layers(
  graph: ReturnType<typeof impactGraph>["graph"],
): Record<string, number | undefined> {
  return Object.fromEntries(
    graph.mapNodes((_key, attributes) => [attributes.label, attributes.layer]),
  )
}

describe("impactGraph", () => {
  it("puts the changed definition in the first layer and each caller in the layer of its depth", () => {
    const { graph } = impactGraph([
      anImpact([caller("load", 1), caller("run", 2), caller("main", 3)]),
    ])

    expect(layers(graph)).toEqual({ parse: 0, load: 1, run: 2, main: 3 })
  })

  it("draws an edge from the changed definition to each direct caller, dashed where the call is a guess", () => {
    const { graph } = impactGraph([anImpact([caller("load", 1), caller("guess", 1, "heuristic")])])

    const edges = graph.mapEdges((_key, attributes, _from, _to, from, to) => ({
      from: from.label,
      to: to.label,
      dashed: attributes.dashed,
    }))
    expect(edges).toEqual([
      { from: "parse", to: "load", dashed: false },
      { from: "parse", to: "guess", dashed: true },
    ])
  })

  it("draws an edge to a caller further out only where one node of the layer before could be its callee", () => {
    const single = impactGraph([anImpact([caller("load", 1), caller("run", 2)])]).graph
    const several = impactGraph([
      anImpact([caller("load", 1), caller("other", 1), caller("run", 2)]),
    ]).graph

    const between = (graph: typeof single) =>
      graph.mapEdges((_key, _attributes, _from, _to, from, to) => `${from.label}>${to.label}`)
    expect(between(single)).toEqual(["parse>load", "load>run"])
    expect(between(several)).toEqual(["parse>load", "parse>other"])
  })

  it("marks each stopped definition as a node that says the walk stopped there", () => {
    const { graph } = impactGraph([anImpact([caller("load", 1), caller("hub", 1)], ["hub"])])

    const nodes = graph.mapNodes((_key, attributes) => attributes)
    expect(nodes.find((node) => node.label.startsWith("hub"))).toMatchObject({
      label: "hub (walk stopped: over 200 callers)",
      tone: "danger",
    })
    expect(nodes.find((node) => node.label === "load")?.tone).toBe("pending")
  })

  it("names the symbol behind each node, whatever its label says", () => {
    const { graph, symbols } = impactGraph([anImpact([caller("hub", 1)], ["hub"])])

    expect(symbols.get(definitionKey(caller("hub", 1)))).toBe("hub")
    expect([...symbols.values()].sort()).toEqual(["hub", "parse"])
    expect(graph.order).toBe(2)
  })

  it("keeps one node for a definition several changed definitions reach, in its nearer layer", () => {
    const other = {
      ...anImpact([caller("shared", 2)]),
      symbol: { ...anImpact([]).symbol, line: 40 },
    }

    const { graph } = impactGraph([anImpact([caller("shared", 1)]), other])

    expect(graph.order).toBe(3)
    expect(layers(graph)).toMatchObject({ shared: 1 })
  })

  it("has only the changed definition where nothing calls it", () => {
    const { graph } = impactGraph([anImpact([])])

    expect(graph.order).toBe(1)
    expect(graph.size).toBe(0)
  })
})
