// @vitest-environment jsdom

/** The Symbols tab against the public knowledge HTTP surface (022). */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type {
  KnowledgeGraphDto,
  KnowledgeHitDto,
  KnowledgeStatusDto,
  KnowledgeSymbolDto,
  RepositoryDto,
} from "@/api"
import { dispatchDomainEvent } from "@/events/dispatch"
import { aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { KnowledgeScreen } from "./knowledge-screen"

vi.mock("@/features/knowledge/graph/sigma-canvas", () => import("@/test/sigma-canvas"))

const WEB: RepositoryDto = aRepository({
  id: "01JREPO000000000000000WEB",
  path: "/home/me/dev/web",
  base_branch: "main",
})
const API: RepositoryDto = aRepository({
  id: "01JREPO000000000000000API",
  path: "/home/me/dev/api",
  base_branch: "trunk",
})
const STATUS: KnowledgeStatusDto = {
  repository_id: WEB.id,
  state: "idle",
  refs: [],
  files: 4,
  symbols: 8,
  languages: [],
  error: null,
}
const HIT: KnowledgeHitDto = {
  repository_id: WEB.id,
  path: "src/render.ts",
  line: 10,
  kind: "function",
  name: "render",
  signature: "function render()",
}

/** What the Path field suggests from: the file graph of the ref. */
const FILES: KnowledgeGraphDto = {
  repository_id: WEB.id,
  git_ref: "main",
  nodes: [
    { path: "src/render.ts", language: "typescript", symbols: 3 },
    { path: "src/ui/panel.ts", language: "typescript", symbols: 2 },
    { path: "lib/log.ts", language: "typescript", symbols: 1 },
  ],
  edges: [],
  truncated: false,
  total_nodes: 3,
}

function definition(name: string, overrides: Partial<KnowledgeSymbolDto> = {}): KnowledgeSymbolDto {
  return {
    repository_id: WEB.id,
    path: `src/${name}.ts`,
    start_line: 10,
    end_line: 12,
    kind: "function",
    name,
    signature: `function ${name}()`,
    doc: `Documents ${name}.`,
    ...overrides,
  }
}

const RENDER = definition("render", {
  path: "src/render.ts",
  context: {
    callers: [
      {
        repository_id: API.id,
        path: "src/call-render.ts",
        line: 20,
        name: "callRender",
        confidence: "heuristic",
        step: "name",
        candidates: 2,
      },
    ],
    callees: [],
    implementations: [],
    references: [],
    tests: [],
    more: { callers: 0, callees: 0, implementations: 0, references: 0, tests: 0 },
  },
})
const RENDER_ALTERNATE = definition("render", {
  path: "src/alternate.ts",
  start_line: 30,
  end_line: 33,
  signature: "function render(input: string)",
  doc: "Documents the alternate.",
  context: { callers: [], callees: [], implementations: [], references: [], tests: [] },
})
const CALL_RENDER = definition("callRender", {
  repository_id: API.id,
  path: "src/call-render.ts",
  start_line: 20,
  end_line: 22,
  context: { callers: [], callees: [], implementations: [], references: [], tests: [] },
})

let requests: URL[] = []
/** What `search` answers: a repository whose index is still being built has none. */
let hits: KnowledgeHitDto[] = [HIT]

function withSource(symbol: KnowledgeSymbolDto): KnowledgeSymbolDto {
  return { ...symbol, context: undefined, source: `${symbol.signature} {\n  return true\n}` }
}

beforeEach(() => {
  requests = []
  hits = [HIT]
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(typeof input === "string" ? input : (input as Request).url)
    requests.push(url)
    if (url.pathname === "/v1/repositories") return Promise.resolve(jsonResponse([WEB, API]))
    if (url.pathname.endsWith("/knowledge")) {
      return Promise.resolve(jsonResponse({ ...STATUS, repository_id: url.pathname.split("/")[3] }))
    }
    if (url.pathname === "/v1/knowledge/search") return Promise.resolve(jsonResponse(hits))
    if (url.pathname === "/v1/knowledge/graph") return Promise.resolve(jsonResponse(FILES))
    if (url.pathname === "/v1/knowledge/symbol") {
      const found =
        url.searchParams.get("name") === "callRender" ? [CALL_RENDER] : [RENDER, RENDER_ALTERNATE]
      return Promise.resolve(
        jsonResponse(url.searchParams.get("detail") === "source" ? found.map(withSource) : found),
      )
    }
    return Promise.resolve(jsonResponse([]))
  })
})

describe("the Symbols tab", () => {
  it("searches by the selected kind and path, then opens a compact result", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=symbols" })

    await user.type(await screen.findByRole("combobox", { name: "Search symbols" }), "ren")
    await user.click(screen.getByRole("combobox", { name: "Kind" }))
    await user.click(await screen.findByRole("option", { name: "Function" }))
    await user.type(screen.getByRole("combobox", { name: "Path" }), "src/")
    await user.click(screen.getByRole("button", { name: "Search" }))

    const results = await screen.findByRole("list", { name: "Symbol search results" })
    expect(within(results).getByText("render")).toBeDefined()
    expect(within(results).getByText("function")).toBeDefined()
    expect(within(results).getByText("src/render.ts:10")).toBeDefined()
    // The last search is the one the button sent: the ones before it are the
    // suggestions the name field asked for as it was typed into.
    const search = requests.filter((url) => url.pathname === "/v1/knowledge/search").at(-1)
    expect(Object.fromEntries(search?.searchParams ?? [])).toMatchObject({
      q: "ren",
      repository: WEB.id,
      git_ref: "main",
      kind: "function",
      path: "src/",
    })

    await user.click(within(results).getByRole("button"))
    expect(new URLSearchParams(location.url.split("?")[1]).get("symbol")).toBe("render")
    expect(await screen.findByRole("button", { name: "callRender ↗" })).toBeDefined()
    // Two fields typed into, each answering every keystroke with its
    // suggestions: more than the default five seconds under a loaded suite.
  }, 15_000)

  it("refetches a search that found nothing once indexing finishes", async () => {
    hits = []
    const user = userEvent.setup()
    // The seed pins the shipped `staleTime`: only the event can refetch.
    const { queryClient } = renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=symbols",
      seed: () => {},
    })

    await user.type(await screen.findByRole("combobox", { name: "Search symbols" }), "ren")
    await user.click(screen.getByRole("button", { name: "Search" }))
    expect(await screen.findByText("No symbols match this search.")).toBeDefined()

    hits = [HIT]
    act(() => {
      dispatchDomainEvent(queryClient, {
        event: "knowledge_indexed",
        data: {
          repository_id: WEB.id,
          git_ref: "main",
          commit: "abc1230000000000000000000000000000000000",
          files: 4,
          symbols: 8,
        },
      })
    })

    const results = await screen.findByRole("list", { name: "Symbol search results" })
    expect(within(results).getByText("src/render.ts:10")).toBeDefined()
    expect(screen.queryByText("No symbols match this search.")).toBeNull()
  }, 15_000)

  it("opens the URL symbol, re-centres on a node, and moves backward and forward", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=symbols&symbol=render",
    })

    await user.click(await screen.findByRole("button", { name: "callRender ↗" }))
    await waitFor(() => expect(screen.getByText("function callRender()")).toBeDefined())
    expect(new URLSearchParams(location.url.split("?")[1]).get("symbol")).toBe("callRender")
    expect(new URLSearchParams(location.url.split("?")[1]).get("repository")).toBe(API.id)

    await user.click(screen.getByRole("button", { name: "Back symbol" }))
    await waitFor(() => expect(screen.getByText("function render()")).toBeDefined())
    expect(new URLSearchParams(location.url.split("?")[1]).get("symbol")).toBe("render")

    await user.click(screen.getByRole("button", { name: "Forward symbol" }))
    await waitFor(() => expect(screen.getByText("function callRender()")).toBeDefined())
  })

  it("shows the centre source with its original line numbers", async () => {
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=symbols&symbol=render" })

    const details = await screen.findByRole("complementary", { name: "Symbol details" })
    expect(within(details).getByText("src/render.ts:10-12")).toBeDefined()
    expect(within(details).getByText("Documents render.")).toBeDefined()
    expect(within(details).getByLabelText("Source").textContent).toContain("10 function render()")
    expect(
      requests.some(
        (url) =>
          url.pathname === "/v1/knowledge/symbol" && url.searchParams.get("detail") === "source",
      ),
    ).toBe(true)
  })

  it("suggests the symbols the search finds under what is typed", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=symbols" })

    await user.type(await screen.findByRole("combobox", { name: "Search symbols" }), "ren")

    await waitFor(() =>
      expect(screen.getAllByRole("option").map((row) => row.textContent)).toEqual([
        "renderfunctionsrc/render.ts",
      ]),
    )
  })

  it("suggests the directories of the ref before its files in the path field", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=symbols" })

    await user.type(await screen.findByRole("combobox", { name: "Path" }), "src")

    await waitFor(() =>
      expect(screen.getAllByRole("option").map((row) => row.textContent)).toEqual([
        "src/directory",
        "src/ui/directory",
        "src/render.tsfile",
        "src/ui/panel.tsfile",
      ]),
    )
  })

  it("lets the user pick one of several definitions", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=symbols&symbol=render" })

    await user.click(await screen.findByRole("combobox", { name: "Definition" }))
    await user.click(await screen.findByRole("option", { name: "src/alternate.ts:30" }))

    const details = screen.getByRole("complementary", { name: "Symbol details" })
    expect(within(details).getByText("function render(input: string)")).toBeDefined()
    expect(within(details).getByText("src/alternate.ts:30-33")).toBeDefined()
  })
})
