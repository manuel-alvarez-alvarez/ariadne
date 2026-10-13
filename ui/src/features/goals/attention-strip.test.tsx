// @vitest-environment jsdom

/**
 * The one row shape the strip cannot answer with a `Link`: a `drafts`
 * item's target is the request's own page on the forge, which Ariadne has
 * no panel of its own to open — so the row itself must be the external
 * link (`ui/AGENTS.md`), not a client-side route that goes nowhere.
 */

import type { QueryClient } from "@tanstack/react-query"
import { screen, waitFor } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import type { AttentionItemDto } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { AttentionStrip } from "./attention-strip"

let recoveryItems: AttentionItemDto[]

function aDraftsItem(): AttentionItemDto {
  return {
    id: "pull_request:drafts:pull-1",
    producer: "drafts",
    reason: "unknown",
    summary: "acme/widgets#1 Fix widgets holds 2 unsubmitted draft comments waiting on the forge.",
    required_action: "Finish and submit it yourself, directly on the forge.",
    since: "2026-01-01T00:00:00Z",
    affected: [{ kind: "pull_request", id: "pull-1", label: "acme/widgets#1 Fix widgets" }],
    target: { kind: "forge", url: "https://github.com/acme/widgets/pull/1" },
  }
}

beforeEach(() => {
  recoveryItems = []
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const { pathname } = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body =
      pathname === "/v1/attention"
        ? { items: recoveryItems, complete: true }
        : pathname === "/v1/goals" || pathname === "/v1/tasks"
          ? []
          : { sessions: [], next_cursor: null, total: 0, snapshot_at: "2026-01-01T00:00:00Z" }
    return Promise.resolve(jsonResponse(body))
  })
})

async function settled(queryClient: QueryClient) {
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
}

it("renders a drafts item's row as a plain link to its own page on the forge", async () => {
  recoveryItems = [aDraftsItem()]
  const { queryClient } = renderScreen(<AttentionStrip />, { route: "/goals" })
  await settled(queryClient)

  const row = await screen.findByText(
    "acme/widgets#1 Fix widgets holds 2 unsubmitted draft comments waiting on the forge.",
  )
  const anchor = row.closest("a")
  expect(anchor).not.toBeNull()
  expect(anchor?.getAttribute("href")).toBe("https://github.com/acme/widgets/pull/1")
})
