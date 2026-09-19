/**
 * The Files tab's graph model (022): a node per file or per directory, sized
 * by its symbols and coloured by its top-level directory, edges weighted by
 * their count and summed across a directory, and filters that hide without a
 * rebuild — fast enough at 5000 files.
 */

import { describe, expect, it } from "vitest"

import type { KnowledgeGraphDto } from "@/api"

import { type FilesFilters, filesGraph, filesVisibility } from "./files-graph"

/** The summed edge between two nodes, of one kind, found by what it joins. */
function between(built: ReturnType<typeof filesGraph>, from: string, to: string, kind: string) {
  return [...built.edges.values()].find(
    (edge) => edge.from === from && edge.to === to && edge.kind === kind,
  )
}

const NO_FILTER: FilesFilters = { text: "", hiddenKinds: new Set(), hideIsolated: false }

/**
 * src/app/main.rs → src/app/config.rs (calls ×3, exact)
 * src/app/main.rs → src/util.rs (imports ×1, heuristic)
 * src/app/config.rs → src/util.rs (calls ×2, exact)
 * tests/it.rs → src/app/main.rs (calls ×4, exact)
 * README.md, on its own.
 */
function aRepository(): KnowledgeGraphDto {
  return {
    repository_id: "01JREPO000000000000000WEB",
    git_ref: "main",
    nodes: [
      { path: "src/app/main.rs", language: "rust", symbols: 16 },
      { path: "src/app/config.rs", language: "rust", symbols: 4 },
      { path: "src/util.rs", language: "rust", symbols: 1 },
      { path: "tests/it.rs", language: "rust", symbols: 9 },
      { path: "README.md", language: "markdown", symbols: 0 },
    ],
    edges: [
      {
        from: "src/app/main.rs",
        to: "src/app/config.rs",
        kind: "calls",
        count: 3,
        confidence: "exact",
      },
      {
        from: "src/app/main.rs",
        to: "src/util.rs",
        kind: "imports",
        count: 1,
        confidence: "heuristic",
      },
      {
        from: "src/app/config.rs",
        to: "src/util.rs",
        kind: "calls",
        count: 2,
        confidence: "exact",
      },
      { from: "tests/it.rs", to: "src/app/main.rs", kind: "calls", count: 4, confidence: "exact" },
    ],
    truncated: false,
    total_nodes: 5,
  }
}

describe("the file level", () => {
  it("has a node per file, named by its last segment", () => {
    const { graph } = filesGraph(aRepository(), "file", 1)

    expect(graph.nodes()).toEqual([
      "src/app/main.rs",
      "src/app/config.rs",
      "src/util.rs",
      "tests/it.rs",
      "README.md",
    ])
    expect(graph.getNodeAttribute("src/app/main.rs", "label")).toBe("main.rs")
  })

  it("draws a file with more symbols bigger", () => {
    const { graph } = filesGraph(aRepository(), "file", 1)

    const size = (node: string) => graph.getNodeAttribute(node, "size") ?? 0
    expect(size("src/app/main.rs")).toBeGreaterThan(size("src/app/config.rs"))
    expect(size("src/app/config.rs")).toBeGreaterThan(size("README.md"))
  })

  it("colours the files of one top-level directory alike, and names each colour in the legend", () => {
    const { graph, legend } = filesGraph(aRepository(), "file", 1)

    const tone = (node: string) => graph.getNodeAttribute(node, "tone")
    expect(tone("src/app/main.rs")).toBe(tone("src/util.rs"))
    expect(tone("tests/it.rs")).not.toBe(tone("src/util.rs"))
    expect(tone("README.md")).not.toBe(tone("tests/it.rs"))
    // Largest directory first: src holds three files.
    expect(legend.map((entry) => entry.label)).toEqual(["src", "(root)", "tests"])
    expect(legend[0]?.tone).toBe(tone("src/util.rs"))
  })

  it("puts the directories past the seventh under one Other colour", () => {
    const dto = aRepository()
    dto.nodes = Array.from({ length: 9 }, (_, index) => ({
      path: `d${index}/a.rs`,
      language: "rust",
      symbols: 1,
    }))
    dto.edges = []

    const { graph, legend } = filesGraph(dto, "file", 1)

    expect(legend).toHaveLength(8)
    expect(legend.at(-1)?.label).toBe("Other")
    expect(graph.getNodeAttribute("d7/a.rs", "tone")).toBe(legend.at(-1)?.tone)
    expect(graph.getNodeAttribute("d8/a.rs", "tone")).toBe(legend.at(-1)?.tone)
  })

  it("gives each edge its count as its weight, and dashes a heuristic one", () => {
    const { graph } = filesGraph(aRepository(), "file", 1)

    const calls = graph.edges("tests/it.rs", "src/app/main.rs")[0] ?? ""
    expect(graph.getEdgeAttribute(calls, "weight")).toBe(4)
    expect(graph.getEdgeAttribute(calls, "dashed")).toBe(false)
    const imports = graph.edges("src/app/main.rs", "src/util.rs")[0] ?? ""
    expect(graph.getEdgeAttribute(imports, "dashed")).toBe(true)
  })

  it("keeps two edges apart when their paths hold the characters a key could join on", () => {
    const dto = aRepository()
    dto.nodes = ["x→y", "z", "x", "y→z"].map((path) => ({ path, language: "rust", symbols: 1 }))
    dto.edges = [
      { from: "x→y", to: "z", kind: "k", count: 1, confidence: "exact" },
      { from: "x", to: "y→z", kind: "k", count: 1, confidence: "exact" },
      { from: "x", to: "z", kind: "a:k", count: 1, confidence: "exact" },
      { from: "x", to: "z:a", kind: "k", count: 1, confidence: "exact" },
    ]
    dto.nodes.push({ path: "z:a", language: "rust", symbols: 1 })

    const { graph } = filesGraph(dto, "file", 1)

    expect(graph.size).toBe(4)
  })

  it("lists every edge kind in the response once, sorted", () => {
    expect(filesGraph(aRepository(), "file", 1).kinds).toEqual(["calls", "imports"])
  })
})

describe("the directory level", () => {
  it("merges the files of one directory into one node, cut to one segment", () => {
    const { graph } = filesGraph(aRepository(), "directory", 1)

    expect(graph.nodes().sort()).toEqual(["(root)", "src", "tests"])
  })

  it("cuts a directory to the depth asked for", () => {
    const { graph } = filesGraph(aRepository(), "directory", 2)

    expect(graph.nodes().sort()).toEqual(["(root)", "src", "src/app", "tests"])
  })

  it("sums the edges between two directories and drops the ones inside one", () => {
    const built = filesGraph(aRepository(), "directory", 2)

    // src/app → src: calls ×2 (config → util); imports ×1 (main → util) stays its own kind.
    expect(between(built, "src/app", "src", "calls")?.count).toBe(2)
    expect(between(built, "src/app", "src", "imports")?.count).toBe(1)
    expect(between(built, "tests", "src/app", "calls")?.count).toBe(4)
    // main → config is inside src/app.
    expect(built.graph.size).toBe(3)
  })

  it("sums the symbols of a directory's files into its size", () => {
    const one = filesGraph(aRepository(), "directory", 1).graph
    const files = filesGraph(aRepository(), "file", 1).graph

    expect(one.getNodeAttribute("src", "size")).toBeGreaterThan(
      files.getNodeAttribute("src/app/main.rs", "size") ?? 0,
    )
  })

  it("marks a summed edge heuristic when one of its edges is", () => {
    const dto = aRepository()
    dto.edges.push({
      from: "tests/it.rs",
      to: "src/app/config.rs",
      kind: "calls",
      count: 1,
      confidence: "heuristic",
    })

    const built = filesGraph(dto, "directory", 1)

    expect(between(built, "tests", "src", "calls")).toMatchObject({
      count: 5,
      confidence: "heuristic",
    })
  })
})

describe("the filters", () => {
  it("hide nothing when nothing is filtered", () => {
    const built = filesGraph(aRepository(), "file", 1)

    const hidden = filesVisibility(built, NO_FILTER)

    expect(hidden.nodes.size).toBe(0)
    expect(hidden.edges.size).toBe(0)
  })

  it("hide a node whose path lacks the text, in any case, and its edges", () => {
    const built = filesGraph(aRepository(), "file", 1)

    const hidden = filesVisibility(built, { ...NO_FILTER, text: "SRC/" })

    expect([...hidden.nodes].sort()).toEqual(["README.md", "tests/it.rs"])
    expect([...hidden.edges].map((key) => built.edges.get(key))).toEqual([
      between(built, "tests/it.rs", "src/app/main.rs", "calls"),
    ])
  })

  it("hide an edge of a kind turned off", () => {
    const built = filesGraph(aRepository(), "file", 1)

    const hidden = filesVisibility(built, { ...NO_FILTER, hiddenKinds: new Set(["calls"]) })

    expect(hidden.nodes.size).toBe(0)
    expect([...hidden.edges].every((key) => built.edges.get(key)?.kind === "calls")).toBe(true)
    expect(hidden.edges.size).toBe(3)
  })

  it("hide a node with no shown edge when unlinked nodes are hidden", () => {
    const built = filesGraph(aRepository(), "file", 1)

    const hidden = filesVisibility(built, {
      ...NO_FILTER,
      hiddenKinds: new Set(["imports"]),
      hideIsolated: true,
    })

    expect([...hidden.nodes]).toEqual(["README.md"])
  })

  it("leave the model as it was built", () => {
    const built = filesGraph(aRepository(), "file", 1)
    const before = built.graph.export()

    filesVisibility(built, { text: "tests", hiddenKinds: new Set(["calls"]), hideIsolated: true })

    expect(built.graph.export()).toEqual(before)
  })
})

describe("a large repository", () => {
  /** 5000 files in 50 directories, each calling the next three files. */
  function aLargeRepository(): KnowledgeGraphDto {
    const nodes = Array.from({ length: 5000 }, (_, index) => ({
      path: `src/d${index % 50}/sub${index % 7}/f${index}.rs`,
      language: "rust",
      symbols: index % 40,
    }))
    const edges = nodes.flatMap((node, index) =>
      [1, 2, 3].map((step) => ({
        from: node.path,
        to: nodes[(index + step) % nodes.length]?.path ?? node.path,
        kind: step === 3 ? "imports" : "calls",
        count: step,
        confidence: "exact" as const,
      })),
    )
    return {
      repository_id: "01JREPO000000000000000WEB",
      git_ref: "main",
      nodes,
      edges,
      truncated: false,
      total_nodes: 5000,
    }
  }

  it("builds both levels of 5000 files and filters them within the time budget", () => {
    const dto = aLargeRepository()

    const started = performance.now()
    const files = filesGraph(dto, "file", 1)
    const directories = filesGraph(dto, "directory", 3)
    filesVisibility(files, { text: "d1", hiddenKinds: new Set(["imports"]), hideIsolated: true })
    const elapsed = performance.now() - started

    expect(files.graph.order).toBe(5000)
    expect(files.graph.size).toBe(15_000)
    expect(directories.graph.order).toBe(350)
    // A frame budget would be too tight for a loaded test runner; a second is not.
    expect(elapsed).toBeLessThan(1000)
  })
})
