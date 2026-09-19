// @vitest-environment jsdom

/**
 * The knowledge page against a stubbed daemon (022): the status card in
 * every state the daemon can answer with, a Reindex button that flips the
 * card to `indexing` at once and settles on the real state once
 * `knowledge_indexed` arrives, a search box that reaches the daemon's search
 * route with its filters, and the interactions section grouped by kind.
 *
 * The event path is driven through the app's own `EventStreamProvider`
 * rather than `dispatchDomainEvent` directly, the way `task-diff.test.tsx`
 * drives the diff tab's own refresh: what is asserted is the whole path, a
 * daemon pushing the event down the one `EventSource` and the card
 * refetching with nothing clicked, not just what the event does to the
 * cache (`@/events/dispatch.test.ts` pins that half).
 */

import { act, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { RepositoryDto } from "@/api"
import { EventStreamProvider } from "@/events/provider"
import { type FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { KnowledgePage } from "./knowledge-page"
import type {
  KnowledgeInteractionGroupDto,
  KnowledgeSearchResultDto,
  KnowledgeStatusDto,
} from "./types"

const REPOSITORY: RepositoryDto = aRepository({ id: "01JREPO00000000000000ARI" })

function idleStatus(overrides: Partial<KnowledgeStatusDto> = {}): KnowledgeStatusDto {
  return {
    repository_id: REPOSITORY.id,
    state: "idle",
    refs: [
      {
        git_ref: "main",
        commit: "abc1230000000000000000000000000000000000",
        indexed_at: "2026-01-01T00:00:00Z",
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

interface Recorded {
  method: string
  url: string
}

let requests: Recorded[] = []
let status: KnowledgeStatusDto = idleStatus()
let searchResults: KnowledgeSearchResultDto[] = []
let interactionGroups: KnowledgeInteractionGroupDto[] = []

function stubDaemon() {
  requests = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    requests.push({ method: request.method, url: url.pathname + url.search })

    if (request.method === "POST" && url.pathname.endsWith("/knowledge/reindex")) {
      return new Response(null, { status: 202 })
    }
    if (url.pathname === "/v1/repositories") return jsonResponse([REPOSITORY])
    if (url.pathname === `/v1/repositories/${REPOSITORY.id}/knowledge`) return jsonResponse(status)
    if (url.pathname === "/v1/knowledge/search") return jsonResponse(searchResults)
    if (url.pathname === "/v1/knowledge/interactions") return jsonResponse(interactionGroups)
    return jsonResponse([])
  })
}

/** The last request made to the search route, if any. */
function lastSearchRequest(): Recorded | undefined {
  return requests.filter((request) => request.url.startsWith("/v1/knowledge/search")).at(-1)
}

beforeEach(() => {
  status = idleStatus()
  searchResults = []
  interactionGroups = []
  stubDaemon()
})

describe("the status card", () => {
  it.each([
    ["idle", idleStatus(), "Idle"],
    ["indexing", idleStatus({ state: "indexing" }), "Indexing"],
    [
      "failed",
      idleStatus({ state: "failed", error: "git clone failed: permission denied" }),
      "Failed",
    ],
    ["disabled", idleStatus({ state: "disabled" }), "Disabled"],
  ] as const)("renders while %s", async (_label, given, badge) => {
    status = given
    renderScreen(<KnowledgePage repositoryId={REPOSITORY.id} />)

    expect(await screen.findByText(badge)).toBeDefined()

    if (given.state === "disabled") {
      expect(
        screen.getByText("Knowledge indexing is turned off for this repository."),
      ).toBeDefined()
      return
    }

    expect(screen.getByText("128")).toBeDefined()
    expect(screen.getByText("512")).toBeDefined()
    expect(screen.getByText("Rust")).toBeDefined()
    if (given.state === "failed") {
      expect(screen.getByText("git clone failed: permission denied")).toBeDefined()
    }
  })
})

describe("reindexing", () => {
  it("posts to the reindex route and shows the indexing state at once", async () => {
    const user = userEvent.setup()
    renderScreen(<KnowledgePage repositoryId={REPOSITORY.id} />)
    await screen.findByText("Idle")

    await user.click(screen.getByRole("button", { name: "Reindex" }))

    expect(await screen.findByText("Indexing")).toBeDefined()
    const post = requests.find((request) => request.method === "POST")
    expect(post?.url).toBe(`/v1/repositories/${REPOSITORY.id}/knowledge/reindex`)
  })

  it("refetches the status once the daemon says indexing finished", async () => {
    stubEventSource()
    renderScreen(
      <EventStreamProvider>
        <KnowledgePage repositoryId={REPOSITORY.id} />
      </EventStreamProvider>,
      { route: "/repositories" },
    )
    expect(await screen.findByText("128")).toBeDefined()

    const stream: FakeEventSource = latestSource()
    act(() => {
      stream.succeed()
      stream.beat()
    })

    status = idleStatus({ files: 777, symbols: 999 })
    act(() => {
      stream.emit("knowledge_indexed", {
        repository_id: REPOSITORY.id,
        git_ref: "main",
        commit: "def4560000000000000000000000000000000000",
        files: 777,
        symbols: 999,
      })
    })

    expect(await screen.findByText("777")).toBeDefined()
    expect(screen.getByText("999")).toBeDefined()
  })
})

describe("search", () => {
  it("calls the search route with q, kind and path, and renders the rows", async () => {
    searchResults = [
      {
        repository_id: REPOSITORY.id,
        path: "src/lib.rs",
        line: 42,
        kind: "function",
        name: "parse_goal",
        signature: "fn parse_goal(input: &str) -> Goal",
      },
    ]
    const user = userEvent.setup()
    renderScreen(<KnowledgePage repositoryId={REPOSITORY.id} />)
    await screen.findByText("Idle")

    await user.type(screen.getByLabelText("Search knowledge"), "parse_goal")
    await user.type(screen.getByLabelText("Filter by kind"), "function")
    await user.type(screen.getByLabelText("Filter by path"), "src/")

    expect(await screen.findByText("parse_goal")).toBeDefined()
    expect(screen.getByText("src/lib.rs:42")).toBeDefined()
    expect(screen.getByText("function")).toBeDefined()
    expect(screen.getByText("fn parse_goal(input: &str) -> Goal")).toBeDefined()

    const last = lastSearchRequest()
    const params = new URLSearchParams(last?.url.split("?")[1])
    expect(params.get("repository")).toBe(REPOSITORY.id)
    expect(params.get("q")).toBe("parse_goal")
    expect(params.get("kind")).toBe("function")
    expect(params.get("path")).toBe("src/")
  })

  it("asks nothing of the search route before a filter is given", async () => {
    renderScreen(<KnowledgePage repositoryId={REPOSITORY.id} />)
    await screen.findByText("Idle")

    expect(lastSearchRequest()).toBeUndefined()
    expect(screen.getByText("Type a query, or a kind or path filter, to search.")).toBeDefined()
  })
})

describe("interactions", () => {
  it("groups edges by kind, showing both ends and their confidence", async () => {
    interactionGroups = [
      {
        kind: "depends_on",
        edges: [
          {
            from: { repository_id: REPOSITORY.id, path: "src/main.rs", line: 10, symbol: "main" },
            to: { repository_id: REPOSITORY.id, path: "src/lib.rs", line: 1, symbol: "run" },
            confidence: "exact",
            step: "path",
            candidates: 1,
          },
        ],
      },
      {
        kind: "calls_route",
        edges: [
          {
            from: {
              repository_id: REPOSITORY.id,
              path: "src/client.ts",
              line: 5,
              symbol: "fetchGoal",
            },
            to: {
              repository_id: "01JREPO0000000000000OTHER",
              path: "src/routes.rs",
              line: 88,
              symbol: "get_goal",
            },
            confidence: "heuristic",
            step: "route",
            candidates: 3,
          },
        ],
      },
    ]
    renderScreen(<KnowledgePage repositoryId={REPOSITORY.id} />)

    expect(await screen.findByText("Depends on")).toBeDefined()
    expect(screen.getByText("Calls route")).toBeDefined()
    expect(screen.getByText(`${REPOSITORY.id}:src/main.rs:10 main`)).toBeDefined()
    expect(screen.getByText(`${REPOSITORY.id}:src/lib.rs:1 run`)).toBeDefined()
    expect(screen.getByText("Exact")).toBeDefined()
    expect(screen.getByText("Heuristic")).toBeDefined()
  })

  it("names the step each edge rests on beside its confidence", async () => {
    interactionGroups = [
      {
        kind: "calls_route",
        edges: [
          {
            from: {
              repository_id: REPOSITORY.id,
              path: "src/client.ts",
              line: 5,
              symbol: "fetchGoal",
            },
            to: {
              repository_id: "01JREPO0000000000000OTHER",
              path: "src/routes.rs",
              line: 88,
              symbol: "get_goal",
            },
            confidence: "heuristic",
            step: "route",
            candidates: 3,
          },
        ],
      },
    ]
    renderScreen(<KnowledgePage repositoryId={REPOSITORY.id} />)

    expect(await screen.findByText("via route")).toBeDefined()
  })

  it("says there is nothing to show before any interaction is found", async () => {
    renderScreen(<KnowledgePage repositoryId={REPOSITORY.id} />)

    expect(await screen.findByText("No interactions found yet.")).toBeDefined()
  })
})
