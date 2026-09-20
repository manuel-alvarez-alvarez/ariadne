// @vitest-environment jsdom

/**
 * The Impact and path tab against a stubbed daemon (022): the callers in
 * layers with the stopped marks, the path as a chain with its edge kinds and
 * its empty state, the inputs in the URL, the suggestions, and the click that
 * opens the Symbols tab.
 *
 * The graph is drawn by the sigma stand-in (`@/test/sigma-canvas.tsx`) and
 * laid out by ELK itself, run on this thread in place of its web worker
 * (`@/test/elk-worker.ts`): a node's `data-x` is where ELK put it.
 */

import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type {
  KnowledgeHitDto,
  KnowledgeImpactCallerDto,
  KnowledgeImpactDto,
  KnowledgePathDto,
  RepositoryDto,
} from "@/api"
import { aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { KnowledgeScreen } from "./knowledge-screen"

// jsdom has no WebGL and no Worker: see `@/test/sigma-canvas.tsx` and `@/test/elk-worker.ts`.
vi.mock("@/features/knowledge/graph/sigma-canvas", () => import("@/test/sigma-canvas"))
vi.mock("elkjs/lib/elk-worker.min.js?worker", () => import("@/test/elk-worker"))

const WEB: RepositoryDto = aRepository({
  id: "01JREPO000000000000000WEB",
  path: "/home/me/dev/web",
  base_branch: "main",
})

function definition(name: string) {
  return { repository_id: WEB.id, path: `src/${name}.rs`, line: 3, name }
}

function caller(
  name: string,
  depth: number,
  confidence: "exact" | "heuristic" = "exact",
): KnowledgeImpactCallerDto {
  return { ...definition(name), depth, confidence, step: "file", candidates: 1 }
}

const IMPACT: KnowledgeImpactDto[] = [
  {
    symbol: { ...definition("parse"), confidence: "exact", step: null, candidates: 1 },
    callers: [caller("load", 1), caller("guess", 1, "heuristic"), caller("hub", 2)],
    stopped: ["hub"],
  },
]

const PATH: KnowledgePathDto = {
  hops: [
    { ...definition("fetchGoal"), kind: "function" },
    { ...definition("get_goal"), kind: "function", edge_kind: "calls_route", confidence: "exact" },
    { ...definition("load_goal"), kind: "function", edge_kind: "calls", confidence: "heuristic" },
  ],
}

const HITS: KnowledgeHitDto[] = [
  { ...definition("parse"), kind: "function", signature: "fn parse()" },
  { ...definition("parse_all"), kind: "function", signature: "fn parse_all()" },
]

let requests: URL[] = []
let impact: KnowledgeImpactDto[] = []
let path: KnowledgePathDto = { hops: [] }

beforeEach(() => {
  requests = []
  impact = IMPACT
  path = PATH
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    requests.push(url)
    if (url.pathname === "/v1/repositories") return jsonResponse([WEB])
    if (url.pathname === "/v1/knowledge/impact") return jsonResponse(impact)
    if (url.pathname === "/v1/knowledge/path") return jsonResponse(path)
    if (url.pathname === "/v1/knowledge/search") return jsonResponse(HITS)
    return jsonResponse({ repository_id: WEB.id, state: "idle", refs: [], files: 0, symbols: 0 })
  })
})

const BASE = `/knowledge?repository=${WEB.id}&tab=impact`

/**
 * The graph, once ELK has laid it out. The first layout loads the engine, and
 * under a loaded suite that takes longer than a query's own second.
 */
function graphDrawn(): Promise<HTMLElement> {
  return screen.findByRole("list", { name: "Graph nodes" }, { timeout: 10_000 })
}

/**
 * What a test gets when it both types a name and waits for a layout: the
 * suggestion list answers every keystroke, and ELK is slower than the
 * default five seconds under a loaded suite either way.
 */
const SLOW = 15_000

function node(name: string): HTMLElement {
  return within(screen.getByRole("list", { name: "Graph nodes" })).getByRole("button", { name })
}

function edges(): HTMLElement[] {
  return within(screen.getByRole("list", { name: "Graph edges" })).getAllByRole("button")
}

function x(name: string): number {
  return Number(node(name).dataset.x)
}

function params(url: string): URLSearchParams {
  return new URLSearchParams(url.split("?")[1])
}

function asked(pathname: string): URLSearchParams[] {
  return requests.filter((url) => url.pathname === pathname).map((url) => url.searchParams)
}

describe("the Impact mode", () => {
  it(
    "draws the callers in layers by depth, from the changed symbol out",
    async () => {
      renderScreen(<KnowledgeScreen />, { route: `${BASE}&symbol=parse&depth=3` })

      await graphDrawn()
      expect(x("parse")).toBeLessThan(x("load"))
      expect(x("guess")).toBeGreaterThan(x("parse"))
      expect(x("hub (walk stopped: over 200 callers)")).toBeGreaterThan(x("load"))
      expect(x("hub (walk stopped: over 200 callers)")).toBeGreaterThan(x("guess"))
      expect(asked("/v1/knowledge/impact")).toHaveLength(1)
      expect(Object.fromEntries(asked("/v1/knowledge/impact")[0] ?? [])).toEqual({
        repository: WEB.id,
        git_ref: "main",
        symbol: "parse",
        depth: "3",
      })
    },
    SLOW,
  )

  it("draws a heuristic call dashed and an exact one as a line", async () => {
    renderScreen(<KnowledgeScreen />, { route: `${BASE}&symbol=parse` })

    await graphDrawn()
    expect(edges().map((edge) => [edge.textContent, edge.dataset.type])).toEqual([
      ["parse → load", "line"],
      ["parse → guess", "dashed"],
    ])
  })

  it("marks a stopped definition as a node that says the walk stopped there", async () => {
    renderScreen(<KnowledgeScreen />, { route: `${BASE}&symbol=parse` })

    await graphDrawn()
    expect(node("hub (walk stopped: over 200 callers)")).toBeDefined()
    expect(screen.getByRole("list", { name: "Legend" }).textContent).toContain(
      "Walk stopped: over 200 callers",
    )
  })

  it("asks for a symbol before it asks the daemon anything", async () => {
    renderScreen(<KnowledgeScreen />, { route: BASE })

    expect(
      await screen.findByText("Name a symbol to see what a change to it reaches."),
    ).toBeDefined()
    expect(asked("/v1/knowledge/impact")).toHaveLength(0)
  })

  it("says so where no definition has the name", async () => {
    impact = []
    renderScreen(<KnowledgeScreen />, { route: `${BASE}&symbol=nothing` })

    expect(await screen.findByText("No definition named nothing at main.")).toBeDefined()
  })

  it("offers a depth from 1 to 4", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: `${BASE}&symbol=parse` })

    await user.click(await screen.findByRole("combobox", { name: "Depth" }))

    expect((await screen.findAllByRole("option")).map((option) => option.textContent)).toEqual([
      "Depth 1",
      "Depth 2",
      "Depth 3",
      "Depth 4",
    ])
  })

  it("suggests the symbols the search finds under what is typed, with their kind and path", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: BASE })

    await user.type(await screen.findByRole("combobox", { name: "Symbol" }), "pa")

    await waitFor(() =>
      expect(screen.getAllByRole("option").map((row) => row.textContent)).toEqual([
        "parsefunctionsrc/parse.rs",
        "parse_allfunctionsrc/parse_all.rs",
      ]),
    )
    expect(asked("/v1/knowledge/search").at(-1)?.get("q")).toBe("pa")
    expect(asked("/v1/knowledge/search").at(-1)?.get("repository")).toBe(WEB.id)
  })

  it("puts the suggestion the arrow keys reach into the field, and applies it as typed text is", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, { route: BASE })
    const field = await screen.findByRole("combobox", { name: "Symbol" })

    await user.type(field, "pa")
    await screen.findAllByRole("option")
    await user.keyboard("{ArrowDown}{Enter}")

    expect((field as HTMLInputElement).value).toBe("parse")
    await user.click(screen.getByRole("button", { name: "Show impact" }))
    expect(params(location.url).get("symbol")).toBe("parse")
  })

  it("closes the list on Escape and keeps what was typed", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: BASE })
    const field = await screen.findByRole("combobox", { name: "Symbol" })

    await user.type(field, "pa")
    await screen.findAllByRole("option")
    await user.keyboard("{Escape}")

    await waitFor(() => expect(screen.queryAllByRole("option")).toHaveLength(0))
    expect((field as HTMLInputElement).value).toBe("pa")
  })
})

describe("the Path mode", () => {
  const PATH_ROUTE = `${BASE}&mode=path&from=fetchGoal&to=load_goal`

  it("draws the hops as a chain, each edge labelled by its edge kind", async () => {
    renderScreen(<KnowledgeScreen />, { route: PATH_ROUTE })

    await graphDrawn()
    expect(x("fetchGoal")).toBeLessThan(x("get_goal"))
    expect(x("get_goal")).toBeLessThan(x("load_goal"))
    // An edge is named only while the pointer is on one of its ends (022),
    // and the middle hop is an end of both.
    fireEvent.mouseEnter(node("get_goal"))
    expect(
      edges().map((edge) => [edge.textContent, edge.dataset.label, edge.dataset.type]),
    ).toEqual([
      ["fetchGoal → get_goal", "calls_route", "line"],
      ["get_goal → load_goal", "calls", "dashed"],
    ])
    expect(Object.fromEntries(asked("/v1/knowledge/path")[0] ?? [])).toEqual({
      repository: WEB.id,
      git_ref: "main",
      from: "fetchGoal",
      to: "load_goal",
      depth: "6",
    })
  })

  it("says there is no path where the daemon finds none", async () => {
    path = { hops: [] }
    renderScreen(<KnowledgeScreen />, { route: `${PATH_ROUTE}&depth=4` })

    expect(
      await screen.findByText("No path from fetchGoal to load_goal within depth 4."),
    ).toBeDefined()
    expect(screen.queryByRole("list", { name: "Graph nodes" })).toBeNull()
  })

  it("asks for both symbols before it asks the daemon anything", async () => {
    renderScreen(<KnowledgeScreen />, { route: `${BASE}&mode=path&from=fetchGoal` })

    expect(
      await screen.findByText("Name two symbols to find how the first reaches the second."),
    ).toBeDefined()
    expect(asked("/v1/knowledge/path")).toHaveLength(0)
  })

  it("offers a depth from 1 to 10", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: PATH_ROUTE })

    await user.click(await screen.findByRole("combobox", { name: "Depth" }))

    expect(await screen.findAllByRole("option")).toHaveLength(10)
  })
})

describe("the URL", () => {
  it(
    "keeps the mode and its inputs, so a reload draws the same graph",
    async () => {
      const user = userEvent.setup()
      const first = renderScreen(<KnowledgeScreen />, { route: BASE })
      await user.click(await screen.findByRole("button", { name: "Path" }))
      await user.type(screen.getByRole("combobox", { name: "From symbol" }), "fetchGoal")
      await user.type(screen.getByRole("combobox", { name: "To symbol" }), "load_goal")
      await user.click(screen.getByRole("button", { name: "Find path" }))
      await user.click(screen.getByRole("combobox", { name: "Depth" }))
      await user.click(await screen.findByRole("option", { name: "Depth 3" }))
      const kept = first.location.url
      expect(params(kept).get("mode")).toBe("path")
      expect(params(kept).get("from")).toBe("fetchGoal")
      expect(params(kept).get("to")).toBe("load_goal")
      expect(params(kept).get("depth")).toBe("3")
      expect(params(kept).get("tab")).toBe("impact")

      cleanup()
      requests = []
      renderScreen(<KnowledgeScreen />, { route: kept })

      expect(await screen.findByRole("button", { name: "Path", pressed: true })).toBeDefined()
      expect(
        (screen.getByRole("combobox", { name: "From symbol" }) as HTMLInputElement).value,
      ).toBe("fetchGoal")
      expect((screen.getByRole("combobox", { name: "To symbol" }) as HTMLInputElement).value).toBe(
        "load_goal",
      )
      expect(screen.getByRole("combobox", { name: "Depth" }).textContent).toContain("Depth 3")
      await graphDrawn()
      expect(asked("/v1/knowledge/path")[0]?.get("depth")).toBe("3")
    },
    SLOW,
  )

  it(
    "keeps the symbol of the impact mode",
    async () => {
      const user = userEvent.setup()
      const { location } = renderScreen(<KnowledgeScreen />, { route: BASE })

      await user.type(await screen.findByRole("combobox", { name: "Symbol" }), "parse")
      await user.click(screen.getByRole("button", { name: "Show impact" }))

      expect(params(location.url).get("symbol")).toBe("parse")
      await graphDrawn()
    },
    SLOW,
  )

  it("falls back to the default depth where the URL holds one the mode does not allow", async () => {
    renderScreen(<KnowledgeScreen />, { route: `${BASE}&symbol=parse&depth=7` })

    await graphDrawn()
    expect(asked("/v1/knowledge/impact")[0]?.get("depth")).toBe("2")
  })
})

describe("a click on a node", () => {
  it("opens the Symbols tab on that symbol, at the same repository and ref", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, {
      route: `${BASE}&ref=feature&symbol=parse&depth=3`,
    })
    await graphDrawn()

    await user.click(node("hub (walk stopped: over 200 callers)"))

    const next = params(location.url)
    expect(next.get("tab")).toBe("symbols")
    expect(next.get("symbol")).toBe("hub")
    expect(next.get("repository")).toBe(WEB.id)
    expect(next.get("ref")).toBe("feature")
    expect(next.has("depth")).toBe(false)
    expect(next.has("mode")).toBe(false)
  })
})
