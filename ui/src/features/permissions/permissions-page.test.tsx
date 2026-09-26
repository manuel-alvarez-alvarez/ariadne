// @vitest-environment jsdom

/**
 * The Permissions screen against a stubbed daemon.
 *
 * One card, one settings row: what is worth pinning is that every fact of
 * `GET /v1/permissions/ai` reaches the screen, that each control sends only
 * the field it changed the moment it changed, that the switch is what the
 * Python check gates, that each prompt saves on blur as its own field, that
 * Restore defaults sends all three prompts as null and is disabled once the
 * effective texts already match the built-in ones, and that a refusal is
 * toasted with the daemon's own words rather than swallowed.
 */

import { fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { AiPermissionsStatusDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { anAiPermissionsStatus } from "@/test/fixtures"
import { daemonFetch, errorResponse, renderScreen } from "@/test/harness"
import { PermissionsPage } from "./permissions-page"

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []
/** What the stub answers a write with: the row as it stands after that write. */
let current: AiPermissionsStatusDto = anAiPermissionsStatus()

/** The last write that went out, whatever it was. */
function lastWrite(): Recorded | undefined {
  return requests.filter((one) => one.method !== "GET").at(-1)
}

/**
 * The daemon: `GET /v1/permissions/ai` always answers the current row, so
 * the screen loads. A write merges its body into that row and answers the
 * result — or is refused, as `failure` says, which only ever applies to a
 * write: the screen has to be on the card before a write can be tried against
 * it.
 */
function stubDaemon(failure?: { status: number; code: string; message: string }) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? (JSON.parse(raw) as Record<string, unknown>) : null
    requests.push({ method: request.method, path: pathname, body })

    if (failure && request.method !== "GET") {
      const { status, code, message } = failure
      return errorResponse(status, code, message)
    }
    if (request.method !== "GET") {
      current = { ...current, ...body }
    }
    return new Response(JSON.stringify(current), {
      status: request.method === "POST" ? 202 : 200,
      headers: { "content-type": "application/json" },
    })
  })
}

beforeEach(() => {
  requests = []
  current = anAiPermissionsStatus()
  stubDaemon()
})

it("shows every fact the daemon answered with", async () => {
  current = anAiPermissionsStatus({
    state: "ready",
    installed_release: "v0.1.4",
    latest_release: "v0.1.5",
    weights_present: true,
    endpoint: "http://127.0.0.1:8901",
    last_refresh_at: "2026-01-01T00:00:00Z",
    last_error: null,
  })
  renderScreen(<PermissionsPage />)

  expect(await screen.findByText("Ready")).toBeDefined()
  expect(screen.getByText("v0.1.4")).toBeDefined()
  expect(screen.getByText("v0.1.5")).toBeDefined()
  expect(screen.getByText("Yes")).toBeDefined()
  expect(screen.getByText("http://127.0.0.1:8901")).toBeDefined()
})

it("shows the last error in the error style, once there is one", async () => {
  current = anAiPermissionsStatus({
    state: "failed",
    last_error: "the release document named no wheel",
  })
  renderScreen(<PermissionsPage />)

  const message = await screen.findByText("the release document named no wheel")
  expect(message.className).toContain("text-destructive")
})

describe("the enabled switch", () => {
  it("sends enabled: true the moment it is turned on", async () => {
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />)

    await user.click(await screen.findByRole("switch", { name: "Enable the AI permission model" }))

    await waitFor(() => expect(lastWrite()?.body).toEqual({ enabled: true }))
  })

  it("is disabled with the version found, where Python is too old", async () => {
    current = anAiPermissionsStatus({
      python: { path: "/usr/bin/python3", version: "3.9.1", ok: false },
    })
    renderScreen(<PermissionsPage />)

    const switchEl = await screen.findByRole("switch", { name: "Enable the AI permission model" })
    expect(switchEl.getAttribute("data-disabled")).not.toBeNull()
    expect(await screen.findByText(/The model needs Python 3.10 or newer/)).toBeDefined()
    expect(screen.getByText(/found 3.9.1/)).toBeDefined()
  })

  it("is disabled and says not found, where no interpreter was found at all", async () => {
    current = anAiPermissionsStatus({ python: { path: null, version: null, ok: false } })
    renderScreen(<PermissionsPage />)

    expect(await screen.findByText(/not found/)).toBeDefined()
  })
})

it("sends the checkpoints picked, and nothing else", async () => {
  const user = userEvent.setup()
  renderScreen(<PermissionsPage />)

  await user.click(await screen.findByRole("combobox", { name: "Checkpoints" }))
  await user.click(await screen.findByRole("option", { name: /^All three checkpoints/ }))

  await waitFor(() => expect(lastWrite()?.body).toEqual({ checkpoints: "all" }))
})

it("sends the threshold typed, once the field is left", async () => {
  const user = userEvent.setup()
  renderScreen(<PermissionsPage />)

  const threshold = await screen.findByRole("spinbutton", { name: "Threshold" })
  await user.clear(threshold)
  await user.type(threshold, "0.6")
  await user.tab()

  await waitFor(() => expect(lastWrite()?.body).toEqual({ threshold: 0.6 }))
})

describe("the daily refresh", () => {
  it("sends the time picked", async () => {
    renderScreen(<PermissionsPage />)

    fireEvent.change(await screen.findByLabelText("Daily refresh"), {
      target: { value: "03:30" },
    })

    await waitFor(() => expect(lastWrite()?.body).toEqual({ schedule: "03:30" }))
  })

  it("sends null once it is cleared back to off", async () => {
    current = anAiPermissionsStatus({ schedule: "03:30" })
    renderScreen(<PermissionsPage />)

    fireEvent.change(await screen.findByLabelText("Daily refresh"), { target: { value: "" } })

    await waitFor(() => expect(lastWrite()?.body).toEqual({ schedule: null }))
  })
})

describe("Prompts", () => {
  it("sends exactly the question, once the field is left", async () => {
    const originalQuestion = current.prompts.question
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />)

    const question = await screen.findByLabelText("Question")
    await user.click(question)
    await user.type(question, " really?")
    await user.tab()

    await waitFor(() =>
      expect(lastWrite()).toEqual({
        method: "PUT",
        path: "/v1/permissions/ai",
        body: { question: `${originalQuestion} really?` },
      }),
    )
  })

  it("sends exactly the allow-when text, once the field is left", async () => {
    const originalAllow = current.prompts.allow_criteria
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />)

    const allow = await screen.findByLabelText("Allow when")
    await user.click(allow)
    await user.type(allow, " too")
    await user.tab()

    await waitFor(() =>
      expect(lastWrite()?.body).toEqual({ allow_criteria: `${originalAllow} too` }),
    )
  })

  it("sends exactly the ask-a-person-when text, once the field is left", async () => {
    const originalReview = current.prompts.review_criteria
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />)

    const review = await screen.findByLabelText("Ask a person when")
    await user.click(review)
    await user.type(review, " too")
    await user.tab()

    await waitFor(() =>
      expect(lastWrite()?.body).toEqual({ review_criteria: `${originalReview} too` }),
    )
  })

  it("sends the three prompts as null, on Restore defaults", async () => {
    current = anAiPermissionsStatus({
      prompts: { question: "Changed?", allow_criteria: "Changed", review_criteria: "Changed" },
    })
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />)

    await user.click(await screen.findByRole("button", { name: "Restore defaults" }))

    await waitFor(() =>
      expect(lastWrite()?.body).toEqual({
        question: null,
        allow_criteria: null,
        review_criteria: null,
      }),
    )
  })

  it("is disabled while the prompts already match the built-in ones", async () => {
    renderScreen(<PermissionsPage />)

    const button = (await screen.findByRole("button", {
      name: "Restore defaults",
    })) as HTMLButtonElement
    expect(button.disabled).toBe(true)
  })

  it("is enabled once a prompt no longer matches the built-in one", async () => {
    current = anAiPermissionsStatus({
      prompts: { question: "Changed?", allow_criteria: "Changed", review_criteria: "Changed" },
    })
    renderScreen(<PermissionsPage />)

    const button = (await screen.findByRole("button", {
      name: "Restore defaults",
    })) as HTMLButtonElement
    expect(button.disabled).toBe(false)
  })
})

describe("Refresh", () => {
  it("posts to the refresh endpoint", async () => {
    current = anAiPermissionsStatus({ enabled: true, state: "ready" })
    const user = userEvent.setup()
    renderScreen(<PermissionsPage />)

    await user.click(await screen.findByRole("button", { name: "Refresh" }))

    await waitFor(() =>
      expect(lastWrite()).toMatchObject({ method: "POST", path: "/v1/permissions/ai/refresh" }),
    )
  })

  it("is disabled while the model is off", async () => {
    renderScreen(<PermissionsPage />)

    const button = (await screen.findByRole("button", { name: "Refresh" })) as HTMLButtonElement
    expect(button.disabled).toBe(true)
  })

  it("is disabled while an install is already running", async () => {
    current = anAiPermissionsStatus({ enabled: true, state: "installing" })
    renderScreen(<PermissionsPage />)

    const button = (await screen.findByRole("button", { name: "Refresh" })) as HTMLButtonElement
    expect(button.disabled).toBe(true)
  })
})

it("toasts the daemon's own message on a refusal", async () => {
  current = anAiPermissionsStatus({ enabled: true, state: "ready" })
  stubDaemon({ status: 409, code: "ai_busy", message: "an install is already running" })
  const user = userEvent.setup()
  renderScreen(
    <>
      <Toaster />
      <PermissionsPage />
    </>,
  )

  await user.click(await screen.findByRole("button", { name: "Refresh" }))

  expect(await screen.findByText(/an install is already running/)).toBeDefined()
})
