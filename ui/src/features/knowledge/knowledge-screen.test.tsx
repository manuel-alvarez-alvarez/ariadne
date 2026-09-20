// @vitest-environment jsdom

/**
 * The knowledge screen against a stubbed daemon (022): the pickers and the
 * tab read from and write to the URL, the Overview cards in every state the
 * daemon answers with and their Reindex button, the Repositories graph
 * with the side list an edge click fills, and the Files graph with its
 * level, its filters, its truncation notice and the file pane a click opens.
 *
 * The event path is driven through the app's own `EventStreamProvider`, the
 * way `task-diff.test.tsx` drives the diff tab's refresh: a daemon pushing
 * `knowledge_indexed` down the one `EventSource` and a card refetching with
 * nothing clicked (`@/events/dispatch.test.ts` pins the cache half).
 */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type {
  KnowledgeGraphDto,
  KnowledgeInteractionGroupDto,
  KnowledgeOutlineEntryDto,
  KnowledgeStatusDto,
  RepositoryDto,
} from "@/api"
import { EventStreamProvider } from "@/events/provider"
import { type FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { KnowledgeScreen } from "./knowledge-screen"

// jsdom has no WebGL, and sigma.js reads `WebGLRenderingContext` as its module
// loads: the graph is drawn by the stand-in in `@/test/sigma-canvas.tsx`. It is
// mocked here, in the files that draw a graph, and not in `@/test/setup`: a
// mock in the setup file slows every test file in the suite.
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

function idleStatus(
  repository: RepositoryDto,
  overrides: Partial<KnowledgeStatusDto> = {},
): KnowledgeStatusDto {
  return {
    repository_id: repository.id,
    state: "idle",
    refs: [
      {
        git_ref: repository.base_branch,
        commit: "abc1230000000000000000000000000000000000",
        indexed_at: "2026-01-01T00:00:00Z",
        files: 128,
        symbols: 512,
      },
      {
        git_ref: "feature",
        commit: "def4560000000000000000000000000000000000",
        indexed_at: "2026-01-02T00:00:00Z",
        files: 130,
        symbols: 520,
      },
    ],
    files: 128,
    symbols: 512,
    languages: [
      { language: "Rust", files: 100 },
      { language: "TypeScript", files: 28 },
    ],
    error: null,
    ...overrides,
  }
}

const CALLS: KnowledgeInteractionGroupDto[] = [
  {
    kind: "calls_route",
    edges: [
      {
        from: { repository_id: WEB.id, path: "src/client.ts", line: 5, symbol: "fetchGoal" },
        to: { repository_id: API.id, path: "src/routes.rs", line: 88, symbol: "get_goal" },
        confidence: "heuristic",
        step: "route",
        candidates: 3,
      },
    ],
  },
]

/** web/src/app.ts → web/src/api.ts, web/lib/log.ts on its own. */
function aFileGraph(overrides: Partial<KnowledgeGraphDto> = {}): KnowledgeGraphDto {
  return {
    repository_id: WEB.id,
    git_ref: "main",
    nodes: [
      { path: "src/app.ts", language: "typescript", symbols: 12 },
      { path: "src/api.ts", language: "typescript", symbols: 3 },
      { path: "lib/log.ts", language: "typescript", symbols: 1 },
    ],
    edges: [{ from: "src/app.ts", to: "src/api.ts", kind: "calls", count: 2, confidence: "exact" }],
    truncated: false,
    total_nodes: 3,
    ...overrides,
  }
}

const APP_OUTLINE: KnowledgeOutlineEntryDto[] = [
  { name: "mount", kind: "function", signature: "function mount()", start_line: 3, end_line: 9 },
]

interface Recorded {
  method: string
  url: string
}

let requests: Recorded[] = []
let statuses: Record<string, KnowledgeStatusDto> = {}
let interactions: KnowledgeInteractionGroupDto[] = []
let fileGraph: KnowledgeGraphDto = aFileGraph()

function stubDaemon() {
  requests = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    requests.push({ method: request.method, url: url.pathname + url.search })

    if (request.method === "POST" && url.pathname.endsWith("/knowledge/reindex")) {
      return new Response(null, { status: 202 })
    }
    if (url.pathname === "/v1/repositories") return jsonResponse([WEB, API])
    const status = /^\/v1\/repositories\/([^/]+)\/knowledge$/.exec(url.pathname)
    if (status?.[1]) return jsonResponse(statuses[status[1]])
    if (url.pathname === "/v1/knowledge/interactions") return jsonResponse(interactions)
    if (url.pathname === "/v1/knowledge/graph") return jsonResponse(fileGraph)
    if (url.pathname === "/v1/knowledge/outline") {
      return jsonResponse(url.searchParams.get("path") === "src/app.ts" ? APP_OUTLINE : [])
    }
    return jsonResponse([])
  })
}

function card(name: string): HTMLElement {
  return screen.getByRole("listitem", { name })
}

beforeEach(() => {
  statuses = { [WEB.id]: idleStatus(WEB), [API.id]: idleStatus(API) }
  interactions = []
  fileGraph = aFileGraph()
  stubDaemon()
})

describe("the pickers and the tab", () => {
  it("opens on the Overview tab, the first repository and its base branch", async () => {
    renderScreen(<KnowledgeScreen />, { route: "/knowledge" })

    expect(await screen.findByRole("tab", { name: "Overview", selected: true })).toBeDefined()
    expect(screen.getByRole("combobox", { name: "Repository" }).textContent).toContain("web")
    expect(screen.getByRole("combobox", { name: "Ref" }).textContent).toContain("main")
  })

  it("reads the repository, the ref and the tab back from the URL", async () => {
    interactions = CALLS
    renderScreen(<KnowledgeScreen />, {
      route: `/knowledge?repository=${API.id}&ref=feature&tab=repositories`,
    })

    expect(await screen.findByRole("tab", { name: "Repositories", selected: true })).toBeDefined()
    expect(screen.getByRole("combobox", { name: "Repository" }).textContent).toContain("api")
    expect(screen.getByRole("combobox", { name: "Ref" }).textContent).toContain("feature")
    // The picked repository is read at the picked ref, every other at its base.
    await waitFor(() => {
      const asked = requests
        .filter((request) => request.url.startsWith("/v1/knowledge/interactions"))
        .map((request) => new URLSearchParams(request.url.split("?")[1]))
      expect(asked.find((params) => params.get("repository") === API.id)?.get("git_ref")).toBe(
        "feature",
      )
      expect(asked.find((params) => params.get("repository") === WEB.id)?.has("git_ref")).toBe(
        false,
      )
    })
  })

  it("writes a picked tab, repository and ref into the URL", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, { route: "/knowledge" })
    await screen.findByRole("tab", { name: "Overview", selected: true })

    await user.click(screen.getByRole("tab", { name: "Symbols" }))
    expect(new URLSearchParams(location.url.split("?")[1]).get("tab")).toBe("symbols")
    expect(screen.getByRole("textbox", { name: "Search symbols" })).toBeDefined()

    await user.click(screen.getByRole("combobox", { name: "Repository" }))
    await user.click(await screen.findByRole("option", { name: "api" }))
    expect(new URLSearchParams(location.url.split("?")[1]).get("repository")).toBe(API.id)

    await user.click(screen.getByRole("combobox", { name: "Ref" }))
    await user.click(await screen.findByRole("option", { name: "feature" }))
    const params = new URLSearchParams(location.url.split("?")[1])
    expect(params.get("ref")).toBe("feature")
    expect(params.get("tab")).toBe("symbols")
  })
})

describe("the Overview tab", () => {
  it("shows a card per repository, with its files, symbols, languages and refs", async () => {
    renderScreen(<KnowledgeScreen />, { route: "/knowledge" })

    await waitFor(() => expect(within(card("web")).getByText("Idle")).toBeDefined())
    const web = within(card("web"))
    expect(web.getByText("128")).toBeDefined()
    expect(web.getByText("512")).toBeDefined()
    expect(web.getByText("Rust")).toBeDefined()
    expect(web.getByText("feature")).toBeDefined()
    expect(card("api")).toBeDefined()
  })

  it.each([
    ["indexing", { state: "indexing" }, "Indexing"],
    ["failed", { state: "failed", error: "git clone failed: permission denied" }, "Failed"],
  ] as const)("shows a repository while %s", async (_label, overrides, badge) => {
    statuses[WEB.id] = idleStatus(WEB, overrides)
    renderScreen(<KnowledgeScreen />, { route: "/knowledge" })

    await waitFor(() => expect(within(card("web")).getByText(badge)).toBeDefined())
    if (overrides.state === "failed") {
      expect(within(card("web")).getByText(overrides.error)).toBeDefined()
    }
  })

  it("turns Reindex off for a disabled repository, and says why", async () => {
    statuses[WEB.id] = idleStatus(WEB, { state: "disabled" })
    renderScreen(<KnowledgeScreen />, { route: "/knowledge" })

    await waitFor(() => expect(within(card("web")).getByText("Disabled")).toBeDefined())
    expect(
      within(card("web")).getByText("Knowledge indexing is turned off for this repository."),
    ).toBeDefined()
    expect(screen.getByRole("button", { name: "Reindex web" }).hasAttribute("disabled")).toBe(true)
    expect(screen.getByRole("button", { name: "Reindex api" }).hasAttribute("disabled")).toBe(false)
  })

  it("posts to the reindex route and shows the indexing state at once", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: "/knowledge" })
    await waitFor(() => expect(within(card("api")).getByText("Idle")).toBeDefined())

    await user.click(screen.getByRole("button", { name: "Reindex api" }))

    await waitFor(() => expect(within(card("api")).getByText("Indexing")).toBeDefined())
    const post = requests.find((request) => request.method === "POST")
    expect(post?.url).toBe(`/v1/repositories/${API.id}/knowledge/reindex`)
  })

  it("refetches a card once the daemon says indexing finished", async () => {
    stubEventSource()
    renderScreen(
      <EventStreamProvider>
        <KnowledgeScreen />
      </EventStreamProvider>,
      { route: "/knowledge" },
    )
    await waitFor(() => expect(within(card("web")).getByText("128")).toBeDefined())

    const stream: FakeEventSource = latestSource()
    act(() => {
      stream.succeed()
      stream.beat()
    })

    statuses[WEB.id] = idleStatus(WEB, { files: 777, symbols: 999 })
    act(() => {
      stream.emit("knowledge_indexed", {
        repository_id: WEB.id,
        git_ref: "main",
        commit: "def4560000000000000000000000000000000000",
        files: 777,
        symbols: 999,
      })
    })

    await waitFor(() => expect(within(card("web")).getByText("777")).toBeDefined())
    expect(within(card("web")).getByText("999")).toBeDefined()
  })
})

describe("the Repositories tab", () => {
  it("draws a node per repository and lists an edge's ends when it is clicked", async () => {
    interactions = CALLS
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=repositories" })

    const nodes = await screen.findByRole("list", { name: "Graph nodes" })
    expect(
      within(nodes)
        .getAllByRole("button")
        .map((button) => button.textContent),
    ).toEqual(["web", "api"])

    await user.click(screen.getByRole("button", { name: "web → api" }))

    const ends = within(screen.getByRole("complementary", { name: "Edge ends" }))
    expect(ends.getByText("Calls route: web → api")).toBeDefined()
    expect(ends.getByText("src/client.ts:5")).toBeDefined()
    expect(ends.getByText("fetchGoal")).toBeDefined()
    expect(ends.getByText("src/routes.rs:88")).toBeDefined()
    expect(ends.getByText("get_goal")).toBeDefined()
    expect(ends.getByText("Heuristic")).toBeDefined()
  })

  it("picks a repository when its node is clicked", async () => {
    interactions = CALLS
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=repositories",
    })

    const nodes = await screen.findByRole("list", { name: "Graph nodes" })
    await user.click(within(nodes).getByRole("button", { name: "api" }))

    expect(new URLSearchParams(location.url.split("?")[1]).get("repository")).toBe(API.id)
  })

  it("drops an edge whose kind is turned off", async () => {
    interactions = CALLS
    const user = userEvent.setup()
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=repositories" })
    await screen.findByRole("button", { name: "web → api" })

    await user.click(screen.getByRole("button", { name: "Calls route", pressed: true }))

    expect(screen.queryByRole("button", { name: "web → api" })).toBeNull()
  })

  it("says so when no repository interacts with another", async () => {
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=repositories" })

    expect(await screen.findByText("No interactions between repositories yet.")).toBeDefined()
  })
})

describe("the Files tab", () => {
  function shownNodes(): string[] {
    return within(screen.getByRole("list", { name: "Graph nodes" }))
      .getAllByRole("button")
      .map((button) => button.textContent ?? "")
  }

  function params(url: string): URLSearchParams {
    return new URLSearchParams(url.split("?")[1])
  }

  it("draws a node per file from the graph route, with a legend colour per directory", async () => {
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=files&level=file&ref=feature" })

    await screen.findByRole("list", { name: "Graph nodes" })
    expect(shownNodes()).toEqual(["app.ts", "api.ts", "log.ts"])
    expect(screen.getByRole("button", { name: "app.ts → api.ts" })).toBeDefined()
    const legend = within(screen.getByRole("list", { name: "Legend" }))
    expect(legend.getAllByRole("listitem").map((item) => item.textContent)).toEqual(["src", "lib"])
    const asked = requests.find((request) => request.url.startsWith("/v1/knowledge/graph"))
    expect(params(asked?.url ?? "").get("repository")).toBe(WEB.id)
    expect(params(asked?.url ?? "").get("git_ref")).toBe("feature")
  })

  it("uses depth-two directories when the URL has no level", async () => {
    fileGraph = aFileGraph({
      nodes: [
        { path: "src/app.ts", language: "typescript", symbols: 12 },
        { path: "src/lib/api.ts", language: "typescript", symbols: 3 },
        { path: "docs/guide.md", language: "markdown", symbols: 1 },
      ],
      edges: [
        { from: "src/app.ts", to: "src/lib/api.ts", kind: "calls", count: 2, confidence: "exact" },
      ],
    })

    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=files" })

    await screen.findByRole("list", { name: "Graph nodes" })
    expect(shownNodes().sort()).toEqual(["docs", "src", "src/lib"])
    expect(
      screen.getByText(
        (_, element) => element?.textContent === "Showing 3 of 3 nodes and 1 of 1 edge.",
      ),
    ).toBeDefined()
  })

  it("merges the files into directories at directory level, and keeps the level in the URL", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=files&level=file",
    })
    await screen.findByRole("list", { name: "Graph nodes" })

    await user.click(screen.getByRole("button", { name: "Directories", pressed: false }))

    expect(params(location.url).get("level")).toBe("directory")
    expect(shownNodes()).toEqual(["src", "lib"])
  })

  it("keeps an expanded directory in the URL and collapses it on request", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=files" })
    const nodes = await screen.findByRole("list", { name: "Graph nodes" })

    await user.click(within(nodes).getByRole("button", { name: "src" }))

    expect(params(location.url).get("open")).toBe("src")
    expect(shownNodes().sort()).toEqual(["api.ts", "app.ts", "lib"])

    await user.click(screen.getByRole("button", { name: "Collapse" }))

    expect(params(location.url).has("open")).toBe(false)
    expect(shownNodes()).toEqual(["src", "lib"])
  })

  it("hides the files the path text and the kinds leave out, and keeps both in the URL", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=files&level=file",
    })
    await screen.findByRole("list", { name: "Graph nodes" })

    await user.type(screen.getByRole("textbox", { name: "Filter by path" }), "src/")
    await waitFor(() => expect(shownNodes()).toEqual(["app.ts", "api.ts"]))
    expect(params(location.url).get("filter")).toBe("src/")

    await user.click(screen.getByRole("button", { name: "calls", pressed: true }))
    expect(screen.queryByRole("button", { name: "app.ts → api.ts" })).toBeNull()
    expect(params(location.url).get("hidden_kinds")).toBe("calls")
  })

  it("hides a file with no edge when unlinked files are hidden", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=files&level=file",
    })
    await screen.findByRole("list", { name: "Graph nodes" })

    await user.click(screen.getByRole("button", { name: "Hide unlinked", pressed: false }))

    expect(shownNodes()).toEqual(["app.ts", "api.ts"])
    expect(params(location.url).get("isolated")).toBe("hide")
  })

  it("reads the level and the filters back from the URL", async () => {
    renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=files&level=directory&depth=2&filter=lib",
    })

    await screen.findByRole("list", { name: "Graph nodes" })
    await waitFor(() => expect(shownNodes()).toEqual(["lib"]))
    expect(screen.getByRole("button", { name: "Directories", pressed: true })).toBeDefined()
  })

  it("opens a clicked file's outline and edges, with a link per symbol to the Symbols tab", async () => {
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=files&level=file&ref=feature",
    })
    const nodes = await screen.findByRole("list", { name: "Graph nodes" })

    await user.click(within(nodes).getByRole("button", { name: "app.ts" }))

    expect(params(location.url).get("file")).toBe("src/app.ts")
    const pane = within(screen.getByRole("complementary", { name: "File" }))
    const symbol = await pane.findByRole("link", { name: "mount" })
    const target = params(symbol.getAttribute("href") ?? "")
    expect(target.get("tab")).toBe("symbols")
    expect(target.get("symbol")).toBe("mount")
    expect(target.get("ref")).toBe("feature")
    expect(target.has("file")).toBe(false)
    const outgoing = within(pane.getByRole("region", { name: "Outgoing" }))
    expect(outgoing.getByRole("button", { name: "src/api.ts" })).toBeDefined()
    expect(outgoing.getByText("calls × 2")).toBeDefined()
    expect(pane.getByRole("region", { name: "Incoming" }).textContent).toContain("0 edges")
    const outline = requests.find((request) => request.url.startsWith("/v1/knowledge/outline"))
    expect(params(outline?.url ?? "").get("path")).toBe("src/app.ts")
  })

  it("says how many files it shows when the graph is cut, and raises the limit on request", async () => {
    fileGraph = aFileGraph({ truncated: true, total_nodes: 4500 })
    const user = userEvent.setup()
    const { location } = renderScreen(<KnowledgeScreen />, {
      route: "/knowledge?tab=files&level=file",
    })

    expect(await screen.findByText("Showing 3 of 4500 files.")).toBeDefined()
    await user.click(screen.getByRole("button", { name: "Show up to 4000" }))

    expect(params(location.url).get("limit")).toBe("4000")
    await waitFor(() =>
      expect(
        requests.some(
          (request) =>
            request.url.startsWith("/v1/knowledge/graph") &&
            params(request.url).get("limit") === "4000",
        ),
      ).toBe(true),
    )
  })

  it("shows no notice when the graph holds every file", async () => {
    renderScreen(<KnowledgeScreen />, { route: "/knowledge?tab=files&level=file" })

    await screen.findByRole("list", { name: "Graph nodes" })
    expect(screen.queryByText(/^Showing .* files\.$/)).toBeNull()
  })
})
