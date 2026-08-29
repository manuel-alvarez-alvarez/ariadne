// @vitest-environment jsdom

/**
 * The profile dialog against a stubbed daemon.
 *
 * The model picker is rendered rather than unit-tested because what matters is
 * the interplay: the catalog list is a convenience bolted onto a free-text
 * field, and every test here is one of the ways that must not curdle — a pick
 * must land in the field, typed text must survive to the request untouched,
 * a model naming no agent CLI must be refused before the request, clearing must
 * fall back to the daemon's `default` sentinel, and a dead catalog endpoint
 * must leave the field a plain input rather than a broken combobox.
 *
 * The prompt stack inside this dialog has its own file, next to the field it
 * is: `profile-prompts-field.test.tsx`.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

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
    efforts: [],
  },
  {
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
    efforts: [],
  },
  {
    id: "codex:gpt-5.5-codex",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
    efforts: [],
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

/** The combobox input, once the dialog is up. */
async function modelBox(): Promise<HTMLInputElement> {
  return (await screen.findByRole("combobox", { name: "Model" })) as HTMLInputElement
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

/** The catalog popup, which lives in a portal outside the dialog. */
async function listbox(): Promise<HTMLElement> {
  return await screen.findByRole("listbox", { name: "Models" })
}

beforeEach(() => {
  requests = []
  stubDaemon()
})

describe("the model picker", () => {
  it("lists the catalog with descriptions on click, and a pick fills the field", async () => {
    const user = userEvent.setup()
    renderDialog(null)
    const box = await modelBox()

    await user.click(box)
    const options = await listbox()

    // The catalog whole, grouped one heading per agent CLI.
    expect(within(options).getByText("Claude Code")).toBeDefined()
    expect(within(options).getByText("Codex")).toBeDefined()
    expect(within(options).getByText("Opus tier: deep analysis")).toBeDefined()

    await user.click(within(options).getByText("claude_code:claude-opus-5"))

    expect(box.value).toBe("claude_code:claude-opus-5")
    await waitFor(() => {
      expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
    })
  })

  it("filters on the description too, not just the model id", async () => {
    const user = userEvent.setup()
    renderDialog(null)
    const box = await modelBox()

    await user.type(box, "deep analysis")
    const options = await listbox()

    await waitFor(() => {
      expect(within(options).queryByText("claude_code:claude-fable-5")).toBeNull()
    })
    expect(within(options).getByText("claude_code:claude-opus-5")).toBeDefined()
  })

  it("filters while typing and picks with the keyboard", async () => {
    const user = userEvent.setup()
    renderDialog(null)
    const box = await modelBox()

    await user.type(box, "gpt-5.5")
    const options = await listbox()
    await waitFor(() => {
      expect(within(options).queryByText("claude_code:claude-fable-5")).toBeNull()
    })
    expect(within(options).getByText("codex:gpt-5.5-codex")).toBeDefined()

    await user.keyboard("{ArrowDown}{Enter}")

    expect(box.value).toBe("codex:gpt-5.5-codex")
  })

  it("sends typed free text as the model, matched by nothing in the catalog", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Name"), "custom")
    await user.type(await modelBox(), "opencode:my-weird-model")
    await user.type(screen.getByLabelText("System prompt"), "Do things.")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => {
      expect(lastRequest("POST", "/v1/profiles")).toBeDefined()
    })
    expect(lastRequest("POST", "/v1/profiles")?.body?.model).toBe("opencode:my-weird-model")
  })

  it("refuses a model that names no agent CLI, before the daemon is asked", async () => {
    const user = userEvent.setup()
    renderDialog(null)

    await user.type(screen.getByLabelText("Name"), "custom")
    await user.type(await modelBox(), "claude-opus-5")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    expect(await screen.findByText(/claude_code:claude-opus-5/)).toBeDefined()
    expect(lastRequest("POST", "/v1/profiles")).toBeUndefined()
  })

  it("clears back to auto, which the update spells as its sentinel", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)
    const box = await modelBox()
    expect(box.value).toBe("claude_code:claude-opus-5")

    await user.click(screen.getByRole("button", { name: "Use auto" }))
    expect(box.value).toBe("")

    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastRequest("PUT", `/v1/profiles/${PROFILE.id}`)).toBeDefined()
    })
    expect(lastRequest("PUT", `/v1/profiles/${PROFILE.id}`)?.body?.model).toBe("default")
  })

  it("says nothing about a model nobody touched, so a rename cannot re-pin it", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)
    // The box holds the profile's pin, as it does on every open — and that is
    // exactly what must not travel: `PUT` is a partial update, so re-sending it
    // would overwrite a model moved from the CLI or another window meanwhile.
    expect((await modelBox()).value).toBe("claude_code:claude-opus-5")

    await user.type(screen.getByLabelText("Name"), "-renamed")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    const request = await waitFor(() => {
      const sent = lastRequest("PUT", `/v1/profiles/${PROFILE.id}`)
      if (!sent) throw new Error("nothing written yet")
      return sent
    })
    expect(request.body?.name).toBe("Builder-renamed")
    expect(request.body).not.toHaveProperty("model")
  })

  it("sends a model that was changed, beside the name it was changed with", async () => {
    const user = userEvent.setup()
    renderDialog(PROFILE)
    await waitFor(async () => expect((await modelBox()).value).toBe("claude_code:claude-opus-5"))

    await user.type(screen.getByLabelText("Name"), "-renamed")
    await user.clear(await modelBox())
    await user.type(await modelBox(), "codex:gpt-5.5-codex")
    await user.click(screen.getByRole("button", { name: "Save changes" }))

    await waitFor(() => {
      expect(lastRequest("PUT", `/v1/profiles/${PROFILE.id}`)).toBeDefined()
    })
    expect(lastRequest("PUT", `/v1/profiles/${PROFILE.id}`)?.body).toMatchObject({
      name: "Builder-renamed",
      model: "codex:gpt-5.5-codex",
    })
  })

  it("degrades to a plain free-text field when the catalog cannot be fetched", async () => {
    const user = userEvent.setup()
    stubDaemon({ models: "error" })
    renderDialog(null)
    const box = await modelBox()

    // No catalog: clicking and typing open nothing, and saving still works.
    await user.click(box)
    await user.type(box, "codex:still-typable")
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
    expect(box.value).toBe("codex:still-typable")

    await user.type(screen.getByLabelText("Name"), "offline")
    await user.type(screen.getByLabelText("System prompt"), "Do things.")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => {
      expect(lastRequest("POST", "/v1/profiles")).toBeDefined()
    })
    expect(lastRequest("POST", "/v1/profiles")?.body?.model).toBe("codex:still-typable")
  })
})

describe("dismissing the dialog", () => {
  it("closes straight away once the profile's own briefings have landed", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(PROFILE, onOpenChange)

    // The prefill is what this is about: wait until it is on screen.
    expect((await expandPrompt(user, "Engineer briefing")).value).toBe("Stored engineer briefing.")

    await user.keyboard("{Escape}")

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })

  it("asks before dropping an edited briefing, and keeps it when the answer is no", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(PROFILE, onOpenChange)

    const briefing = await expandPrompt(user, "Engineer briefing")
    await user.type(briefing, " Also: never force-push.")
    await user.keyboard("{Escape}")

    expect(await screen.findByText("Discard changes?")).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Keep editing" }))

    expect((await promptBox("Engineer briefing")).value).toBe(
      "Stored engineer briefing. Also: never force-push.",
    )
    expect(onOpenChange).not.toHaveBeenCalled()
  })
})
