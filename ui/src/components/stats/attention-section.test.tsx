// @vitest-environment jsdom

/**
 * The Attention section against a stubbed daemon: it asks `GET /v1/stats/attention`
 * with the screen's filter, under the key `qk` names, and says its empty
 * sentence under its heading and its question.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { AttentionSection } from "./attention-section"

/** The URLs of every request the section made. */
let asked: URL[] = []

beforeEach(() => {
  asked = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    asked.push(new URL(request.url))
    return jsonResponse({})
  })
})

it("asks for its family with the filter, and says its empty sentence", async () => {
  const filter = { since: "7d", repo: "01JREPO" }
  const { queryClient } = renderScreen(<AttentionSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Attention" })
  expect(within(section).getByRole("heading", { level: 2 }).textContent).toBe("Attention")
  expect(within(section).getByText("How much did it need me?")).toBeDefined()
  expect(await within(section).findByText("Nothing needed you in this span.")).toBeDefined()
  expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/attention"])
  expect(asked[0]?.searchParams.get("since")).toBe("7d")
  expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
  expect(queryClient.getQueryData(qk.stats.attention(filter))).toEqual({})
})
