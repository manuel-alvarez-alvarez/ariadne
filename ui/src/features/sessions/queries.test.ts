import { QueryClient } from "@tanstack/react-query"
import { expect, it } from "vitest"

import { aSession, aSessionPage } from "@/test/fixtures"
import { daemonFetch, jsonResponse } from "@/test/harness"

import { sessionsQueryOptions } from "./queries"

const FIRST = aSession({ id: "01JSESS000000000000000FIRST", worktree_path: "/tmp/wt" })
const SECOND = aSession({ id: "01JSESS00000000000000SECOND" })

/** The query of every `GET /v1/sessions` the test made, oldest first. */
function listings(): URLSearchParams[] {
  return daemonFetch.mock.calls
    .map(([input, init]) => (input instanceof Request ? input : new Request(input, init)))
    .map(({ url }) => new URL(url))
    .filter((url) => url.pathname === "/v1/sessions")
    .map((url) => url.searchParams)
}

it("reads every page of Ariadne's sessions, ended ones too, as session rows", async () => {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const url = new URL((input instanceof Request ? input : new Request(input, init)).url)
    return Promise.resolve(
      jsonResponse(
        url.searchParams.get("cursor") === "page-2"
          ? aSessionPage([SECOND])
          : aSessionPage([FIRST], { next_cursor: "page-2", total: 2 }),
      ),
    )
  })

  const sessions = await new QueryClient().fetchQuery(sessionsQueryOptions({ task: "t1" }))

  expect(sessions).toEqual([FIRST, SECOND])
  const [first, second] = listings()
  expect(first?.get("kind")).toBe("ariadne")
  expect(first?.get("all")).toBe("true")
  expect(first?.get("task")).toBe("t1")
  expect(first?.get("cursor")).toBeNull()
  expect(second?.get("cursor")).toBe("page-2")
  expect(listings()).toHaveLength(2)
})
