/**
 * The Symbols tab's graph model (022): one centre, one coloured edge per
 * relation, one aggregate node per hidden tail, and a visible mark on ends
 * outside the centre's repository.
 */

import { describe, expect, it } from "vitest"

import type { KnowledgeSymbolDto } from "@/api"

import { SYMBOL_RELATIONS, symbolsGraph } from "./symbols-graph"

const WEB = "01JREPO000000000000000WEB"
const API = "01JREPO000000000000000API"

function related(name: string, repositoryId = WEB, confidence: "exact" | "heuristic" = "exact") {
  return {
    repository_id: repositoryId,
    path: `src/${name}.ts`,
    line: 20,
    name,
    confidence,
    step: confidence === "exact" ? "file" : "name",
    candidates: confidence === "exact" ? 1 : 2,
  }
}

const CENTRE: KnowledgeSymbolDto = {
  repository_id: WEB,
  path: "src/render.ts",
  start_line: 10,
  end_line: 14,
  kind: "function",
  name: "render",
  signature: "function render()",
  doc: "Draw the page.",
  context: {
    callers: [related("caller")],
    callees: [related("callee")],
    implementations: [related("implementation")],
    references: [related("reference", API, "heuristic")],
    tests: [related("test")],
    more: { callers: 2, callees: 0, implementations: 3, references: 0, tests: 1 },
  },
}

describe("symbolsGraph", () => {
  it("places the centre and every relation, including more and other repositories", () => {
    const built = symbolsGraph(CENTRE)

    expect(built.graph.getNodeAttributes("centre")).toMatchObject({
      label: "render",
      x: 0,
      y: 0,
    })
    expect(
      SYMBOL_RELATIONS.map((relation) => [
        relation,
        new Set(
          built.graph
            .filterEdges((_edge, attributes) => attributes.label === relation)
            .map((edge) => built.graph.getEdgeAttribute(edge, "tone")),
        ),
      ]),
    ).toEqual([
      ["callers", new Set(["active"])],
      ["callees", new Set(["ready"])],
      ["implementations", new Set(["review"])],
      ["references", new Set(["warn"])],
      ["tests", new Set(["danger"])],
    ])
    expect(built.graph.findNode((_node, attributes) => attributes.label === "+2 more")).toBeTruthy()
    expect(built.graph.findNode((_node, attributes) => attributes.label === "+3 more")).toBeTruthy()
    expect(built.graph.findNode((_node, attributes) => attributes.label === "+1 more")).toBeTruthy()

    const foreign = built.graph.findNode((_node, attributes) => attributes.label === "reference ↗")
    expect(foreign).toBeTruthy()
    expect(foreign ? built.targets.get(foreign)?.repositoryId : undefined).toBe(API)
    const foreignEdge = foreign ? built.graph.edges(foreign, "centre")[0] : undefined
    expect(foreignEdge ? built.graph.getEdgeAttribute(foreignEdge, "dashed") : undefined).toBe(true)
  })
})
