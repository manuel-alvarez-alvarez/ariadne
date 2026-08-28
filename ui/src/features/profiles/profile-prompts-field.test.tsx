// @vitest-environment jsdom

/**
 * The prompt stack inside the profile dialog, against a stubbed daemon.
 *
 * These are here for the opposite reason to the combobox's tests: what the
 * editors must not do is *write*. A profile being created has no prompts to
 * show, so it carries none; editing writes a briefing only when its text moved
 * — reading one that is a default and leaving it alone must never turn it into
 * a text of the profile's own; and restoring a default is the one write that
 * happens on the spot. Each of those is asserted on the requests that reached
 * the stub, because a stray `PUT` is exactly the failure that looks like
 * success on screen.
 *
 * The field is rendered through the dialog that hosts it, because what it does
 * on submit is half of what is under test.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it } from "vitest"

import type { ModelDto, ProfileDto, ProfilePromptDto } from "@/api"
import { aProfile } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { ProfileFormDialog } from "./profile-form-dialog"

const STAMP = "2026-01-01T00:00:00Z"

const PROFILE: ProfileDto = aProfile({
  id: "01JPROF000000000000000ENG",
  name: "Builder",
  model: "claude_code:claude-opus-5",
  system_prompt: "Stored system prompt.",
})

/** The default of the one briefing these tests restore. */
const DEFAULT_BRIEFING = "Default engineer briefing."

/** The default system prompt the reset endpoint answers with. */
const DEFAULT_SYSTEM = "Default engineer system prompt."

/**
 * What the edited profile is briefed with: one briefing it was given, one it
 * was not — which the daemon answers with the default of the kind and the flag
 * saying so.
 */
const STORED_PROMPTS: ProfilePromptDto[] = [
  {
    kind: "engineer_briefing",
    content: "Stored engineer briefing.",
    is_default: false,
    updated_at: STAMP,
  },
  {
    kind: "changes_requested",
    content: "Default changes requested.",
    is_default: true,
    updated_at: null,
  },
]

/** A slice of the daemon's curated catalog, two agents wide. */
const CATALOG: ModelDto[] = [
  {
    id: "claude_code:claude-fable-5",
    agent_kind: "claude_code",
    description: "Frontier: highest capability",
  },
  {
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
  },
  {
    id: "codex:gpt-5.5-codex",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
  },
]

interface Recorded {
  method: string
  path: string
  body: {
    name?: string
    model?: string | null
    system_prompt?: string | null
    content?: string
    prompts?: { kind: string; content: string }[]
  } | null
}

let requests: Recorded[] = []

/** The last request that went to `path` with `method`, or undefined. */
function lastRequest(method: string, path: string): Recorded | undefined {
  return requests.filter((one) => one.method === method && one.path === path).at(-1)
}

/** Every request the dialog makes with `path`, in the order they went out. */
function requestsTo(method: string, path: string): Recorded[] {
  return requests.filter((one) => one.method === method && one.path === path)
}

/**
 * The reads the dialog does plus the writes it can make, echoing like the
 * daemon. `promptFailure` turns one briefing's `PUT` into a 500, which is the
 * partial failure the dialog has to survive.
 */
function stubDaemon({
  models = CATALOG,
  promptFailure,
}: {
  models?: ModelDto[] | "error"
  promptFailure?: string
} = {}) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    if (pathname === "/v1/models") {
      return models === "error" ? jsonResponse({ error: "boom" }, 500) : jsonResponse(models)
    }
    if (pathname === `/v1/profiles/${PROFILE.id}/prompts`) {
      return jsonResponse(STORED_PROMPTS)
    }
    const promptReset = pathname.match(/^\/v1\/profiles\/[^/]+\/prompts\/([a-z_]+)\/reset$/)
    if (promptReset && request.method === "POST") {
      return jsonResponse({
        kind: promptReset[1],
        content: DEFAULT_BRIEFING,
        is_default: true,
        updated_at: null,
      })
    }
    if (pathname === `/v1/profiles/${PROFILE.id}/system-prompt/reset`) {
      return jsonResponse({
        ...PROFILE,
        system_prompt: DEFAULT_SYSTEM,
        system_prompt_is_default: true,
      })
    }
    const promptWrite = pathname.match(/^\/v1\/profiles\/[^/]+\/prompts\/([a-z_]+)$/)
    if (promptWrite && request.method === "PUT") {
      const kind = promptWrite[1]
      return kind === promptFailure
        ? jsonResponse({ error: "prompt write failed" }, 500)
        : jsonResponse({ kind, content: body?.content ?? "", is_default: false, updated_at: STAMP })
    }
    if (pathname === "/v1/profiles" && request.method === "POST") {
      return jsonResponse({ ...PROFILE, ...body, id: "01JPROF00000000000000NEW" })
    }
    if (pathname === `/v1/profiles/${PROFILE.id}` && request.method === "PUT") {
      return jsonResponse({ ...PROFILE, ...body })
    }
    return new Response("not stubbed", { status: 404 })
  })
}

function renderDialog(
  profile: ProfileDto | null,
  onOpenChange: (open: boolean) => void = () => {},
) {
  return renderScreen(<ProfileFormDialog open onOpenChange={onOpenChange} profile={profile} />)
}

/** One prompt's textarea, which is only in the tree while its section is open. */
async function promptBox(label: string): Promise<HTMLTextAreaElement> {
  return (await screen.findByRole("textbox", { name: label })) as HTMLTextAreaElement
}

/** Folds a prompt section open and answers the textarea inside it. */
async function expandPrompt(
  user: ReturnType<typeof userEvent.setup>,
  label: string,
): Promise<HTMLTextAreaElement> {
  await user.click(await screen.findByRole("button", { name: `Expand ${label}` }))
  return await promptBox(label)
}

beforeEach(() => {
  requests = []
  stubDaemon()
})

describe("the prompt editors", () => {
  it("offers a new profile the system prompt alone, blank for its role's own", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    const system = await promptBox("System prompt")
    expect(system.value).toBe("")
    // Nothing to read yet: a profile that does not exist owns no prompts, and
    // the defaults are the daemon's text with no endpoint handing them out.
    expect(screen.queryByRole("button", { name: "Expand Engineer briefing" })).toBeNull()
    expect(requests.every((one) => one.path !== "/v1/profiles//prompts")).toBe(true)

    await user.type(screen.getByLabelText("Name"), "builder")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => {
      expect(lastRequest("POST", "/v1/profiles")).toBeDefined()
    })
    const body = lastRequest("POST", "/v1/profiles")?.body
    expect(body?.system_prompt).toBeNull()
    expect(body).not.toHaveProperty("prompts")
    expect(requests.filter((one) => one.method === "PUT")).toEqual([])
  })

  it("sends a system prompt typed at creation as the profile's own", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Name"), "builder")
    await user.type(screen.getByLabelText("System prompt"), "Mine.")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => {
      expect(lastRequest("POST", "/v1/profiles")).toBeDefined()
    })
    expect(lastRequest("POST", "/v1/profiles")?.body?.system_prompt).toBe("Mine.")
  })

  it("shows an edited profile every prompt, saying which are its own", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)

    expect((await expandPrompt(user, "Engineer briefing")).value).toBe("Stored engineer briefing.")
    expect((await expandPrompt(user, "Changes requested")).value).toBe("Default changes requested.")
    // The briefing it was given can be restored; the one it was not is already
    // its default, so there is nothing to drop.
    expect(
      screen.getByRole("button", { name: "Restore Engineer briefing to default" }),
    ).toHaveProperty("disabled", false)
    expect(
      screen.getByRole("button", { name: "Restore Changes requested to default" }),
    ).toHaveProperty("disabled", true)
  })

  it("edits by writing the one briefing that moved, and nothing else", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)

    const briefing = await expandPrompt(user, "Changes requested")
    expect(briefing.value).toBe("Default changes requested.")
    await user.type(briefing, " Again.")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(
        requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/changes_requested`),
      ).toHaveLength(1)
    })
    expect(
      requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/changes_requested`)[0]?.body?.content,
    ).toBe("Default changes requested. Again.")
    // Untouched prompts, and untouched profile fields, are not written at all.
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/engineer_briefing`)).toEqual([])
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toEqual([])
  })

  it("writes nothing for a default that was only read: submitting leaves it alone", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)

    // Reading a default must not quietly make it the profile's own text.
    await expandPrompt(user, "Changes requested")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastRequest("PUT", `/v1/profiles/${PROFILE.id}`)).toBeUndefined()
    })
    expect(requests.filter((one) => one.method !== "GET")).toEqual([])
  })

  it("restores a default on the spot, and the submit that follows re-writes nothing", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)

    const briefing = await expandPrompt(user, "Engineer briefing")
    expect(briefing.value).toBe("Stored engineer briefing.")

    await user.click(screen.getByRole("button", { name: "Restore Engineer briefing to default" }))

    // The default is the daemon's text, so restoring one is its own request —
    // and what comes back is what fills the box.
    await waitFor(() => {
      expect(briefing.value).toBe(DEFAULT_BRIEFING)
    })
    expect(
      requestsTo("POST", `/v1/profiles/${PROFILE.id}/prompts/engineer_briefing/reset`),
    ).toHaveLength(1)

    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(requests.filter((one) => one.method === "POST")).toHaveLength(1)
    })
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/engineer_briefing`)).toEqual([])
  })

  it("restores the system prompt the same way, off the profile itself", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)

    const system = await promptBox("System prompt")
    expect(system.value).toBe("Stored system prompt.")

    await user.click(screen.getByRole("button", { name: "Restore System prompt to default" }))

    await waitFor(() => {
      expect(system.value).toBe(DEFAULT_SYSTEM)
    })
    expect(requestsTo("POST", `/v1/profiles/${PROFILE.id}/system-prompt/reset`)).toHaveLength(1)
    // And it now reads as the default, so there is nothing left to restore.
    expect(screen.getByRole("button", { name: "Restore System prompt to default" })).toHaveProperty(
      "disabled",
      true,
    )
  })

  it("leaves a restored system prompt alone when the next save is about something else", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)

    await user.click(screen.getByRole("button", { name: "Restore System prompt to default" }))
    await waitFor(() => {
      expect((screen.getByLabelText("System prompt") as HTMLTextAreaElement).value).toBe(
        DEFAULT_SYSTEM,
      )
    })

    await user.type(screen.getByLabelText("Name"), "-v2")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
    })
    // The box holds the default now, and sending it back would store it as
    // this profile's own text — undoing the restore that just happened.
    const body = requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body
    expect(body).not.toHaveProperty("system_prompt")
    expect(body?.name).toBe("Builder-v2")
  })

  it("keeps the dialog up when one write fails, and retries only what is left", async () => {
    const user = userEvent.setup()
    stubDaemon({ promptFailure: "changes_requested" })
    renderDialog(PROFILE)

    await user.type(screen.getByLabelText("Name"), "-v2")
    const changes = await expandPrompt(user, "Changes requested")
    await user.type(changes, " Again.")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    // The failure names the write that did not land, and the dialog stays up.
    const alert = await screen.findByRole("alert")
    expect(alert.textContent).toContain("changes requested")
    expect(screen.getByRole("button", { name: "Save changes" })).toBeDefined()
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)

    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(
        requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/changes_requested`),
      ).toHaveLength(2)
    })
    // The profile itself already landed, so the retry leaves it alone.
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
  })
})

/**
 * Five prompt editors deep, an outside press is the most expensive misclick in
 * the app — but the filling of those editors is the dialog's own doing, not the
 * user's, so it must not be what turns a glance into a question.
 */
