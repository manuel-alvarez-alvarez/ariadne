// @vitest-environment jsdom

/**
 * The Attention section against a stubbed daemon: it asks `GET /v1/stats/attention`
 * with the screen's filter, under the key `qk` names, and says its empty
 * sentence under its heading and its question.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { AttentionSection } from "./attention-section"

/**
 * The explanation behind a labelled figure, read off its tooltip on hover. A
 * label such as "Allow" also names a plain `sr-only` table header with no
 * tooltip of its own, so the explained trigger is picked out by the one
 * thing that marks it: its `aria-describedby`.
 */
async function explanationOf(container: HTMLElement, label: string): Promise<string | null> {
  const user = userEvent.setup()
  const trigger = within(container)
    .getAllByText(label)
    .find((el) => el.hasAttribute("aria-describedby"))
  if (!trigger) throw new Error(`no explained trigger for "${label}"`)
  await user.hover(trigger)
  const id = trigger.getAttribute("aria-describedby")
  return id ? (document.getElementById(id)?.textContent ?? null) : null
}

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

it("shows the empty state when every attention figure is zero", async () => {
  daemonFetch.mockImplementation(async () =>
    jsonResponse({
      permissions: { total: 0, person_share: 0, by_decider: [] },
      flags: ["waiting_permission", "waiting_input", "waiting_user", "stalled", "agent_error"].map(
        (reason) => ({ reason, raised: 0, mean_wait_secs: 0 }),
      ),
      sessions_failed: 0,
      sessions_stalled: 0,
      exhaustions: 0,
    }),
  )
  renderScreen(<AttentionSection filter={{}} />, { route: "/stats" })
  expect(await screen.findByText("Nothing needed you in this span.")).toBeDefined()
})

it("draws its tiles and charts from the family response", async () => {
  const filter = { since: "7d", repo: "01JREPO" }
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    asked.push(new URL(request.url))
    return jsonResponse({
      permissions: {
        total: 3,
        person_share: 2 / 3,
        by_decider: [
          {
            decided_by: "console",
            total: 2,
            allowed: 1,
            denied: 1,
            cancelled: 0,
            mean_wait_ms: 1500,
          },
          { decided_by: "auto", total: 1, allowed: 1, denied: 0, cancelled: 0, mean_wait_ms: 0 },
        ],
      },
      flags: [
        { reason: "waiting_permission", raised: 2, mean_wait_secs: 4 },
        { reason: "waiting_input", raised: 1, mean_wait_secs: 8 },
        { reason: "waiting_user", raised: 0, mean_wait_secs: 0 },
        { reason: "stalled", raised: 1, mean_wait_secs: 12 },
        { reason: "agent_error", raised: 0, mean_wait_secs: 0 },
      ],
      sessions_failed: 1,
      sessions_stalled: 1,
      exhaustions: 0,
    })
  })
  const { queryClient } = renderScreen(<AttentionSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Attention" })
  expect(within(section).getByRole("heading", { level: 2 }).textContent).toBe("Attention")
  expect(within(section).getByText("How much did it need me?")).toBeDefined()
  expect(await within(section).findByText("Prompts you answered")).toBeDefined()
  expect(within(section).getByText("Questions asked")).toBeDefined()
  expect(
    within(section).getByRole("table", { name: "Permission answers by decider" }),
  ).toBeDefined()
  expect(within(section).getByRole("table", { name: "Attention flags by reason" })).toBeDefined()
  expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/attention"])
  expect(asked[0]?.searchParams.get("since")).toBe("7d")
  expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
  expect(queryClient.getQueryData(qk.stats.attention(filter))).toMatchObject({
    sessions_stalled: 1,
  })
})

it("explains every tile and every chart series, on hover", async () => {
  daemonFetch.mockImplementation(async () =>
    jsonResponse({
      permissions: {
        total: 2,
        person_share: 1,
        by_decider: [
          { decided_by: "console", total: 2, allowed: 1, denied: 1, cancelled: 0, mean_wait_ms: 0 },
        ],
      },
      flags: [{ reason: "waiting_input", raised: 1, mean_wait_secs: 0 }],
      sessions_failed: 0,
      sessions_stalled: 0,
      exhaustions: 0,
    }),
  )
  renderScreen(<AttentionSection filter={{}} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Attention" })
  await within(section).findByText("Prompts you answered")

  expect(await explanationOf(section, "Prompts you answered")).toBe(
    "Prompts you answered: the count of permission prompts you answered.",
  )
  expect(await explanationOf(section, "Your mean wait")).toBe(
    "Your mean wait: the mean time a permission prompt waited on you.",
  )
  expect(await explanationOf(section, "Questions asked")).toBe(
    "Questions asked: the count of times an agent asked you a question.",
  )
  expect(await explanationOf(section, "Stalled sessions")).toBe(
    "Stalled sessions: the count of sessions that ended stalled or in an agent error.",
  )
  expect(await explanationOf(section, "Allow")).toBe(
    "Allow: permission prompts this decider allowed.",
  )
  expect(await explanationOf(section, "Deny")).toBe("Deny: permission prompts this decider denied.")
  expect(await explanationOf(section, "Cancelled")).toBe(
    "Cancelled: permission prompts this decider cancelled.",
  )
  expect(await explanationOf(section, "Raised")).toBe(
    "Raised: attention flags raised for this reason.",
  )
})
