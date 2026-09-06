// @vitest-environment jsdom

/**
 * The models screen against a stubbed daemon.
 *
 * The catalog is not the user's to edit, so the whole of this screen is one
 * switch per row and what it sends: `PUT /v1/models/enabled` carries the id in
 * its *body*, because a model id carries both `:` and — for the ids opencode
 * discovers — `/`, which no path segment holds.
 *
 * What is worth pinning beyond the round trip is what the screen does with a
 * refusal. The daemon refuses the last model left on, and a switch that
 * springs back with nothing said reads as a click that never registered.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { ModelDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { aModel } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"

import { ModelsPage } from "./models-page"

const OPUS = aModel({
  id: "claude_code:claude-opus-5",
  description: "the frontier model",
  tier: "frontier",
})
/** Discovered by opencode, so its id carries a `/` of its own. */
const LOCAL = aModel({
  id: "opencode:anthropic/claude-sonnet-4",
  agent_kind: "opencode",
  enabled: false,
})

interface Recorded {
  method: string
  path: string
  body: { id?: string; enabled?: boolean } | null
}

let requests: Recorded[] = []

function lastWrite(): Recorded | undefined {
  return requests.filter((one) => one.method !== "GET").at(-1)
}

/**
 * The daemon, storing what it is sent, so the refetch a toggle triggers
 * answers with the new list rather than the old one.
 *
 * `refuse` stands in for the one refusal there is: the last model left on.
 */
function stubDaemon(models: ModelDto[] = [OPUS, LOCAL], refuse?: string) {
  const stored = models.map((model) => ({ ...model }))
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (request.method === "PUT") {
      if (refuse !== undefined) return errorResponse(409, "conflict", refuse)
      const model = stored.find((one) => one.id === body?.id)
      if (!model) return jsonResponse({})
      model.enabled = body?.enabled ?? true
      return jsonResponse(model)
    }
    return jsonResponse(stored)
  })
}

/** The switch on one model's row, by the accessible name it carries. */
function toggle(id: string): Promise<HTMLElement> {
  return screen.findByRole("switch", { name: `${id} available` })
}

beforeEach(() => {
  requests = []
  stubDaemon()
})

it("lists every model with what it is and whether it can be staffed on", async () => {
  renderScreen(<ModelsPage />)

  expect(await screen.findByText(OPUS.id)).toBeDefined()
  expect(screen.getByText("the frontier model")).toBeDefined()
  expect(screen.getByText(LOCAL.id)).toBeDefined()

  // The count says how many are off, which is the thing a reader of this
  // screen came to check and the one fact no single row carries.
  expect(await screen.findByText("2 models, 1 turned off")).toBeDefined()

  expect((await toggle(OPUS.id)).getAttribute("aria-checked")).toBe("true")
  expect((await toggle(LOCAL.id)).getAttribute("aria-checked")).toBe("false")
})

it("sends the id in the body, so a model named with a slash still travels", async () => {
  const user = userEvent.setup()
  renderScreen(<ModelsPage />)

  await user.click(await toggle(LOCAL.id))

  await waitFor(() => expect(lastWrite()).toBeDefined())
  const write = lastWrite()
  expect(write?.method).toBe("PUT")
  // Not `/v1/models/opencode:anthropic/claude-sonnet-4`, which is two
  // segments and matches no route.
  expect(write?.path).toBe("/v1/models/enabled")
  expect(write?.body).toEqual({ id: LOCAL.id, enabled: true })

  await waitFor(async () =>
    expect((await toggle(LOCAL.id)).getAttribute("aria-checked")).toBe("true"),
  )
})

it("turns a model off", async () => {
  const user = userEvent.setup()
  renderScreen(<ModelsPage />)

  await user.click(await toggle(OPUS.id))

  await waitFor(() => expect(lastWrite()?.body).toEqual({ id: OPUS.id, enabled: false }))
  await waitFor(async () =>
    expect((await toggle(OPUS.id)).getAttribute("aria-checked")).toBe("false"),
  )
})

/**
 * The daemon refuses the last model left on. A switch that springs back with
 * nothing said reads as a click that never registered, so the reason it gave
 * is what the screen shows — and the row goes back to what the daemon still
 * says rather than to what was asked for.
 */
it("says why, where the daemon refuses to turn a model off", async () => {
  const refusal = "`claude_code:claude-opus-5` is the last model left enabled"
  stubDaemon([{ ...OPUS, enabled: true }], refusal)
  const user = userEvent.setup()
  renderScreen(
    <>
      <Toaster />
      <ModelsPage />
    </>,
  )

  await user.click(await toggle(OPUS.id))

  expect(await screen.findByText(refusal)).toBeDefined()
  expect((await toggle(OPUS.id)).getAttribute("aria-checked")).toBe("true")
})

it("says what a daemon with no catalog left on screen", async () => {
  stubDaemon([])
  renderScreen(<ModelsPage />)

  expect(await screen.findByText("No models")).toBeDefined()
  expect(screen.getByText(/ariadne models ls/)).toBeDefined()
})
