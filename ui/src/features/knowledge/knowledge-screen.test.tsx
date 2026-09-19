// @vitest-environment jsdom

/**
 * The knowledge screen against a stubbed daemon (022): the pickers and the
 * tab read from and write to the URL, the Overview cards in every state the
 * daemon answers with and their Reindex button, and the Repositories graph
 * with the side list an edge click fills.
 *
 * The event path is driven through the app's own `EventStreamProvider`, the
 * way `task-diff.test.tsx` drives the diff tab's refresh: a daemon pushing
 * `knowledge_indexed` down the one `EventSource` and a card refetching with
 * nothing clicked (`@/events/dispatch.test.ts` pins the cache half).
 */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { KnowledgeInteractionGroupDto, KnowledgeStatusDto, RepositoryDto } from "@/api"
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

interface Recorded {
  method: string
  url: string
}

let requests: Recorded[] = []
let statuses: Record<string, KnowledgeStatusDto> = {}
let interactions: KnowledgeInteractionGroupDto[] = []

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
    return jsonResponse([])
  })
}

function card(name: string): HTMLElement {
  return screen.getByRole("listitem", { name })
}

beforeEach(() => {
  statuses = { [WEB.id]: idleStatus(WEB), [API.id]: idleStatus(API) }
  interactions = []
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
