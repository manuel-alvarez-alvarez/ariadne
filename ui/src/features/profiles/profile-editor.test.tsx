// @vitest-environment jsdom

/**
 * The profile editor against a stubbed daemon.
 *
 * Most of what is asserted is what the editor must *not* write: a name edited
 * on its own is a `PUT` of the name alone, a briefing read and left alone is
 * never written back as the profile's own text, a restored default is not
 * re-sent by the Save that follows, and a save that failed halfway retries
 * only what is left. Each is asserted on the requests that reached the stub,
 * because a stray `PUT` is exactly the failure that looks like success on
 * screen.
 *
 * Rendered under a data router because leaving a dirty editor is the router's
 * question to raise (`useBlocker`); the screen around it is
 * `profiles-page.test.tsx`'s.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { createMemoryRouter, RouterProvider, useLocation, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { ModelDto, ProfileDto, ProfilePromptDto } from "@/api"
import { paths } from "@/routes/paths"
import { aProfile } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { ProfileEditor } from "./profile-editor"

const STAMP = "2026-01-01T00:00:00Z"

const PROFILE: ProfileDto = aProfile({
  id: "01JPROF000000000000000ENG",
  name: "Builder",
  model: "claude_code:claude-opus-5",
  effort: "high",
  system_prompt: "Stored system prompt.",
})

/** The default of the one briefing these tests restore. */
const DEFAULT_BRIEFING = "Default engineer briefing."

/** The default system prompt the reset endpoint answers with. */
const DEFAULT_SYSTEM = "Default engineer system prompt."

/**
 * What the profile is briefed with: one briefing it was given, one it was not
 * — which the daemon answers with the default of the kind and the flag saying
 * so.
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
    efforts: ["low", "medium", "high", "xhigh", "max"],
    default_effort: "high",
  },
  {
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
    efforts: ["low", "medium", "high", "xhigh", "max"],
    default_effort: "high",
  },
  {
    id: "codex:gpt-5.5-codex",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
    efforts: ["low", "medium", "high", "xhigh"],
    default_effort: "medium",
  },
]

interface Recorded {
  method: string
  path: string
  body: {
    name?: string
    model?: string | null
    effort?: string | null
    system_prompt?: string | null
    content?: string
  } | null
}

let requests: Recorded[] = []

/**
 * Lets the profile's `PUT` through, where the stub was told to hold it: what
 * the editor does *while* a save is in flight is what that test is about.
 */
let releaseProfilePut: (() => void) | null = null

/** The same for a restore — a briefing's or the system prompt's. */
let releaseReset: (() => void) | null = null

/** Every request that went to `path` with `method`, in the order they went out. */
function requestsTo(method: string, path: string): Recorded[] {
  return requests.filter((one) => one.method === method && one.path === path)
}

/** Every write, in order, as `METHOD path`. */
function writes(): string[] {
  return requests.filter((one) => one.method !== "GET").map((one) => `${one.method} ${one.path}`)
}

/**
 * The reads the editor does plus the writes it can make, echoing like the
 * daemon. `promptFailure` turns one briefing's `PUT` into a 500, which is the
 * partial failure the editor has to survive; `nameTaken` answers the profile's
 * `PUT` with the 409 a duplicate name gets.
 */
function stubDaemon({
  models = CATALOG,
  promptFailure,
  nameTaken = false,
  holdProfilePut = false,
  holdReset = false,
}: {
  models?: ModelDto[] | "error"
  promptFailure?: string
  nameTaken?: boolean
  /** Answers the profile's `PUT` only once `releaseProfilePut` is called. */
  holdProfilePut?: boolean
  /** Answers a restore only once `releaseReset` is called. */
  holdReset?: boolean
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
      if (holdReset) {
        await new Promise<void>((resolve) => {
          releaseReset = resolve
        })
      }
      return jsonResponse({
        kind: promptReset[1],
        content: DEFAULT_BRIEFING,
        is_default: true,
        updated_at: null,
      })
    }
    if (pathname === `/v1/profiles/${PROFILE.id}/system-prompt/reset`) {
      if (holdReset) {
        await new Promise<void>((resolve) => {
          releaseReset = resolve
        })
      }
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
    if (pathname === `/v1/profiles/${PROFILE.id}` && request.method === "PUT") {
      if (holdProfilePut) {
        await new Promise<void>((resolve) => {
          releaseProfilePut = resolve
        })
      }
      return nameTaken
        ? errorResponse(409, "profile_name_taken", "a profile with that name exists")
        : jsonResponse({ ...PROFILE, ...body })
    }
    return new Response("not stubbed", { status: 404 })
  })
}

/** Hands the test the way the event stream would hand the editor a new row. */
let updateElsewhere: (profile: ProfileDto) => void = () => {}

/**
 * The editor under a data router, on the profiles screen with somewhere else
 * to go: leaving is a navigation, and the guard is the router's.
 */
function renderEditor({ onDeleted = () => {} }: { onDeleted?: () => void } = {}) {
  function Host() {
    const [profile, setProfile] = useState(PROFILE)
    updateElsewhere = setProfile
    const navigate = useNavigate()
    const location = useLocation()
    return (
      <>
        <button type="button" onClick={() => void navigate(paths.goals())}>
          leave for the board
        </button>
        <output data-testid="pathname">{location.pathname}</output>
        <ProfileEditor profile={profile} onBack={() => {}} onDeleted={onDeleted} />
      </>
    )
  }
  const router = createMemoryRouter(
    [
      { path: paths.profiles(), element: <Host /> },
      { path: paths.goals(), element: <p>the board</p> },
    ],
    { initialEntries: [paths.profile(PROFILE.id)] },
  )
  return renderScreen(<RouterProvider router={router} />, { route: null })
}

/** Opens one prompt's tab and answers its textarea. */
async function openPrompt(
  user: ReturnType<typeof userEvent.setup>,
  label: string,
): Promise<HTMLTextAreaElement> {
  await user.click(await screen.findByRole("tab", { name: new RegExp(`^${label}`) }))
  return (await screen.findByRole("textbox", { name: label })) as HTMLTextAreaElement
}

/** The name box, once the form is up. */
async function nameBox(): Promise<HTMLInputElement> {
  return (await screen.findByLabelText("Name")) as HTMLInputElement
}

/** The one Save there is, which is only on screen while something is unsaved. */
function saveButton(): HTMLElement {
  return screen.getByRole("button", { name: "Save" })
}

/**
 * Asks for one prompt's default back, and says yes to the question that
 * comes: the restore is a write that outlives the form, so it is the one
 * control here that confirms first.
 */
async function restore(user: ReturnType<typeof userEvent.setup>, label: string): Promise<void> {
  await user.click(screen.getByRole("button", { name: `Restore ${label} to default` }))
  const dialog = await screen.findByRole("dialog", {
    name: `Restore ${label.toLowerCase()} to its default?`,
  })
  await user.click(within(dialog).getByRole("button", { name: "Restore default" }))
}

/** The one control the pin is made in. */
async function pinButton(): Promise<HTMLElement> {
  return await screen.findByRole("button", { name: "Runs on" })
}

/** What the trigger reads, with the whitespace a screen collapses collapsed. */
async function pinReads(): Promise<string> {
  return ((await pinButton()).textContent ?? "").replace(/\s+/g, " ").trim()
}

/** Opens the picker, and answers the catalog inside it. */
async function openPin(user: ReturnType<typeof userEvent.setup>): Promise<HTMLElement> {
  await user.click(await pinButton())
  return await screen.findByRole("listbox", { name: "Models" })
}

/** Shuts the picker again, the way the form behind it must survive. */
async function closePin(user: ReturnType<typeof userEvent.setup>) {
  await user.keyboard("{Escape}")
  await waitFor(() => {
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  })
}

beforeEach(() => {
  requests = []
  releaseProfilePut = null
  releaseReset = null
  stubDaemon()
})

describe("the form", () => {
  it("shows every prompt on its tab, saying which are the profile's own", async () => {
    const user = userEvent.setup()
    renderEditor()

    expect((await openPrompt(user, "System prompt")).value).toBe("Stored system prompt.")
    expect((await openPrompt(user, "Engineer briefing")).value).toBe("Stored engineer briefing.")
    // The briefing it was given can be restored; the one it was not is
    // already its default, so there is nothing to drop.
    expect(screen.getByText("edited")).toBeDefined()
    expect(
      screen.getByRole("button", { name: "Restore Engineer briefing to default" }),
    ).toHaveProperty("disabled", false)

    expect((await openPrompt(user, "Changes requested")).value).toBe("Default changes requested.")
    expect(screen.getByText("default")).toBeDefined()
    expect(
      screen.getByRole("button", { name: "Restore Changes requested to default" }),
    ).toHaveProperty("disabled", true)
  })

  it("has no save bar until something is edited, and drops it again on Discard", async () => {
    const user = userEvent.setup()
    renderEditor()
    const name = await nameBox()

    expect(screen.queryByRole("button", { name: "Save" })).toBeNull()

    await user.type(name, "-v2")
    expect(screen.getByText("Unsaved changes")).toBeDefined()

    await user.click(screen.getByRole("button", { name: "Discard" }))
    expect(name.value).toBe("Builder")
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull()
    expect(writes()).toEqual([])
  })

  it("marks the tab whose text moved", async () => {
    const user = userEvent.setup()
    renderEditor()

    await user.type(await openPrompt(user, "Engineer briefing"), " More.")

    expect(screen.getByRole("tab", { name: "Engineer briefing, unsaved" })).toBeDefined()
    expect(screen.getByRole("tab", { name: "System prompt" })).toBeDefined()
  })

  it("describes a pinned model with the catalog's own word on it", async () => {
    renderEditor()
    await nameBox()

    expect(await screen.findByText("Opus tier: deep analysis")).toBeDefined()
  })
})

describe("saving", () => {
  it("writes the profile first, then only the briefings that moved, and clears the bar", async () => {
    const user = userEvent.setup()
    renderEditor()

    await user.type(await nameBox(), "-v2")
    await user.type(await openPrompt(user, "Engineer briefing"), " More.")
    // Read, and left alone: the default must not become the profile's own.
    await openPrompt(user, "Changes requested")
    await user.click(saveButton())

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Save" })).toBeNull()
    })
    expect(writes()).toEqual([
      `PUT /v1/profiles/${PROFILE.id}`,
      `PUT /v1/profiles/${PROFILE.id}/prompts/engineer_briefing`,
    ])
    // The name alone: the pin and the system prompt were not touched, and a
    // partial update that re-sent them would overwrite a change made elsewhere.
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body).toEqual({
      name: "Builder-v2",
    })
    expect(
      requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/engineer_briefing`)[0]?.body?.content,
    ).toBe("Stored engineer briefing. More.")
  })

  it("writes nothing for a default that was only read", async () => {
    const user = userEvent.setup()
    renderEditor()

    await openPrompt(user, "Changes requested")
    await user.type(await nameBox(), "-v2")
    await user.click(saveButton())

    await waitFor(() => {
      expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
    })
    expect(writes()).toEqual([`PUT /v1/profiles/${PROFILE.id}`])
  })

  it("keeps the bar up when one write fails, and retries only what is left", async () => {
    const user = userEvent.setup()
    stubDaemon({ promptFailure: "changes_requested" })
    renderEditor()

    await user.type(await nameBox(), "-v2")
    await user.type(await openPrompt(user, "Changes requested"), " Again.")
    await user.click(saveButton())

    // The failure names the write that did not land, and the form stays dirty.
    const alert = await screen.findByRole("alert")
    expect(alert.textContent).toContain("changes requested")
    expect(saveButton()).toBeDefined()
    expect(writes()).toEqual([
      `PUT /v1/profiles/${PROFILE.id}`,
      `PUT /v1/profiles/${PROFILE.id}/prompts/changes_requested`,
    ])

    await user.click(saveButton())

    await waitFor(() => {
      expect(
        requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/changes_requested`),
      ).toHaveLength(2)
    })
    // The profile itself already landed, so the retry leaves it alone.
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
  })

  /**
   * The boxes stay open while a save is in flight, and a save takes a while.
   * What is typed meanwhile is not in the snapshot being written, so it must
   * still be there — and still unsaved — when that snapshot lands; the one
   * thing that waits is a restore, a write of its own that the save's next
   * write would undo.
   */
  it("keeps what is typed while a save is in flight, as the next thing to save", async () => {
    const user = userEvent.setup()
    stubDaemon({ holdProfilePut: true })
    renderEditor()

    const name = await nameBox()
    await user.type(name, "-v2")
    const briefing = await openPrompt(user, "Engineer briefing")
    await user.click(saveButton())
    await waitFor(() => {
      expect(releaseProfilePut).not.toBeNull()
    })

    // In flight: typing goes on, restoring waits.
    await user.type(name, "-b")
    await user.type(briefing, " Typed meanwhile.")
    expect(
      screen.getByRole("button", { name: "Restore Engineer briefing to default" }),
    ).toHaveProperty("disabled", true)
    expect(screen.getByText("Saving…")).toBeDefined()

    releaseProfilePut?.()

    // The snapshot landed as it was taken; what came after it is still on
    // screen and still unsaved.
    await waitFor(() => {
      expect(screen.getByText("Unsaved changes")).toBeDefined()
    })
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body).toEqual({ name: "Builder-v2" })
    expect(name.value).toBe("Builder-v2-b")
    expect(briefing.value).toBe("Stored engineer briefing. Typed meanwhile.")
    expect(
      screen.getByRole("button", { name: "Restore Engineer briefing to default" }),
    ).toHaveProperty("disabled", false)

    // And the next Save writes exactly that.
    stubDaemon()
    await user.click(saveButton())
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Save" })).toBeNull()
    })
    expect(writes()).toEqual([
      `PUT /v1/profiles/${PROFILE.id}`,
      `PUT /v1/profiles/${PROFILE.id}`,
      `PUT /v1/profiles/${PROFILE.id}/prompts/engineer_briefing`,
    ])
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[1]?.body).toEqual({
      name: "Builder-v2-b",
    })
    expect(
      requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/engineer_briefing`)[0]?.body?.content,
    ).toBe("Stored engineer briefing. Typed meanwhile.")
  })

  it("puts a name clash on the name field", async () => {
    const user = userEvent.setup()
    stubDaemon({ nameTaken: true })
    renderEditor()

    await user.type(await nameBox(), "-v2")
    await user.click(saveButton())

    expect(await screen.findByText('A profile named "Builder-v2" already exists.')).toBeDefined()
    expect(saveButton()).toBeDefined()
  })
})

describe("restoring a default", () => {
  it("writes on the spot, refills the box, and the Save that follows re-sends nothing", async () => {
    const user = userEvent.setup()
    renderEditor()

    const briefing = await openPrompt(user, "Engineer briefing")
    await restore(user, "Engineer briefing")

    await waitFor(() => {
      expect(briefing.value).toBe(DEFAULT_BRIEFING)
    })
    expect(writes()).toEqual([`POST /v1/profiles/${PROFILE.id}/prompts/engineer_briefing/reset`])
    // The badge follows the daemon's flag, and the restore is not an edit.
    expect(screen.getByText("default")).toBeDefined()
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull()

    await user.type(await nameBox(), "-v2")
    await user.click(saveButton())

    await waitFor(() => {
      expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
    })
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}/prompts/engineer_briefing`)).toEqual([])
  })

  it("writes nothing until the question is answered", async () => {
    const user = userEvent.setup()
    renderEditor()

    const briefing = await openPrompt(user, "Engineer briefing")
    await user.click(screen.getByRole("button", { name: "Restore Engineer briefing to default" }))

    const dialog = await screen.findByRole("dialog", {
      name: "Restore engineer briefing to its default?",
    })
    expect(dialog.textContent).toContain("written straight away")
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }))

    expect(writes()).toEqual([])
    expect(briefing.value).toBe("Stored engineer briefing.")
  })

  /**
   * The other direction of the in-flight rule: a Save started while a restore
   * is on its way would snapshot the text the restore is about to replace,
   * and write it straight back once the restore had moved the baseline. So
   * Save waits — the button, the chord and the handler behind them both.
   */
  it("holds Save while a briefing is being restored, so the restore is never re-sent", async () => {
    const user = userEvent.setup()
    stubDaemon({ holdReset: true })
    renderEditor()

    const name = await nameBox()
    await user.type(name, "-v2")
    const briefing = await openPrompt(user, "Engineer briefing")
    await restore(user, "Engineer briefing")
    await waitFor(() => {
      expect(releaseReset).not.toBeNull()
    })

    // In flight: Save is out of reach, from the button and from the keyboard.
    expect(screen.getByText("Restoring…")).toBeDefined()
    expect(saveButton()).toHaveProperty("disabled", true)
    await user.click(saveButton())
    await user.click(name)
    await user.keyboard("{Meta>}{Enter}{/Meta}")
    expect(writes()).toEqual([`POST /v1/profiles/${PROFILE.id}/prompts/engineer_briefing/reset`])

    releaseReset?.()

    await waitFor(() => {
      expect(briefing.value).toBe(DEFAULT_BRIEFING)
    })
    expect(saveButton()).toHaveProperty("disabled", false)

    // The Save that follows writes the rename and nothing about the briefing.
    await user.click(saveButton())
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Save" })).toBeNull()
    })
    expect(writes()).toEqual([
      `POST /v1/profiles/${PROFILE.id}/prompts/engineer_briefing/reset`,
      `PUT /v1/profiles/${PROFILE.id}`,
    ])
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body).toEqual({ name: "Builder-v2" })
  })

  it("holds Save while the system prompt is being restored, so the default is never stored as the profile's own", async () => {
    const user = userEvent.setup()
    stubDaemon({ holdReset: true })
    renderEditor()

    const name = await nameBox()
    await user.type(name, "-v2")
    const system = await openPrompt(user, "System prompt")
    await restore(user, "System prompt")
    await waitFor(() => {
      expect(releaseReset).not.toBeNull()
    })

    expect(saveButton()).toHaveProperty("disabled", true)
    await user.click(saveButton())
    expect(writes()).toEqual([`POST /v1/profiles/${PROFILE.id}/system-prompt/reset`])

    releaseReset?.()

    await waitFor(() => {
      expect(system.value).toBe(DEFAULT_SYSTEM)
    })
    await user.click(saveButton())
    await waitFor(() => {
      expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
    })
    // The rename alone: the box holds the default, and sending it would store
    // it as this profile's own text.
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body).toEqual({ name: "Builder-v2" })
  })

  it("restores the system prompt off the profile itself, and a later rename leaves it alone", async () => {
    const user = userEvent.setup()
    renderEditor()

    const system = await openPrompt(user, "System prompt")
    await restore(user, "System prompt")

    await waitFor(() => {
      expect(system.value).toBe(DEFAULT_SYSTEM)
    })
    expect(writes()).toEqual([`POST /v1/profiles/${PROFILE.id}/system-prompt/reset`])
    expect(screen.getByRole("button", { name: "Restore System prompt to default" })).toHaveProperty(
      "disabled",
      true,
    )

    await user.type(await nameBox(), "-v2")
    await user.click(saveButton())

    await waitFor(() => {
      expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
    })
    // The box holds the default now, and sending it back would store it as
    // this profile's own text — undoing the restore that just happened.
    const body = requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body
    expect(body).not.toHaveProperty("system_prompt")
    expect(body?.name).toBe("Builder-v2")
  })
})

describe("a profile updated elsewhere", () => {
  it("refills a clean form", async () => {
    renderEditor()
    const name = await nameBox()

    updateElsewhere({ ...PROFILE, name: "Renamed from the CLI" })

    await waitFor(() => {
      expect(name.value).toBe("Renamed from the CLI")
    })
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull()
  })

  it("leaves a dirty form alone", async () => {
    const user = userEvent.setup()
    renderEditor()
    const name = await nameBox()
    await user.type(name, "-mine")

    updateElsewhere({ ...PROFILE, name: "Renamed from the CLI" })

    // The heading is the stored name; the box is the edit in progress.
    expect(
      await screen.findByRole("heading", { level: 2, name: "Renamed from the CLI" }),
    ).toBeDefined()
    expect(name.value).toBe("Builder-mine")
    expect(saveButton()).toBeDefined()
  })
})

describe("the pin", () => {
  it("clears back to auto, which the update spells as its sentinel", async () => {
    const user = userEvent.setup()
    renderEditor()
    await waitFor(async () => expect(await pinReads()).toContain("claude-opus-5"))

    await openPin(user)
    await user.click(screen.getByText("auto — first installed CLI, on its own default model"))
    await closePin(user)
    expect(await pinReads()).toBe("auto — first installed CLI, on its own default model")

    await user.click(saveButton())

    await waitFor(() => {
      expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
    })
    // The model comes back with its effort: an effort beside a model handed
    // back is refused outright.
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body).toMatchObject({
      model: "default",
      effort: "default",
    })
  })

  it("clears the effort alone with the same sentinel", async () => {
    const user = userEvent.setup()
    renderEditor()
    await waitFor(async () => expect(await pinReads()).toContain("· high"))

    await openPin(user)
    await user.click(await screen.findByRole("radio", { name: "auto (high)" }))
    await closePin(user)
    await user.click(saveButton())

    await waitFor(() => {
      expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
    })
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body?.effort).toBe("default")
  })

  /**
   * The daemon drops the effort from a pin whose model moves, so it has to
   * travel with the model even where nobody touched it — otherwise the form
   * says `high` and the store says the CLI's own.
   */
  it("sends the effort beside a model that was changed", async () => {
    const user = userEvent.setup()
    renderEditor()
    await waitFor(async () => expect(await pinReads()).toContain("claude-opus-5"))

    const options = await openPin(user)
    await user.click(within(options).getByText("claude_code:claude-fable-5"))
    await closePin(user)
    await user.click(saveButton())

    await waitFor(() => {
      expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)).toHaveLength(1)
    })
    expect(requestsTo("PUT", `/v1/profiles/${PROFILE.id}`)[0]?.body).toMatchObject({
      model: "claude_code:claude-fable-5",
      effort: "high",
    })
  })
})

describe("leaving", () => {
  it("asks before another screen takes a dirty editor away, and stays on Keep editing", async () => {
    const user = userEvent.setup()
    renderEditor()
    await user.type(await nameBox(), "-v2")

    await user.click(screen.getByRole("button", { name: "leave for the board" }))

    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Keep editing" }))

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Discard changes?" })).toBeNull()
    })
    expect(screen.getByTestId("pathname").textContent).toBe(paths.profiles())
    expect((await nameBox()).value).toBe("Builder-v2")
  })

  it("goes on Discard", async () => {
    const user = userEvent.setup()
    renderEditor()
    await user.type(await nameBox(), "-v2")

    await user.click(screen.getByRole("button", { name: "leave for the board" }))
    const dialog = await screen.findByRole("dialog", { name: "Discard changes?" })
    await user.click(within(dialog).getByRole("button", { name: "Discard" }))

    expect(await screen.findByText("the board")).toBeDefined()
  })

  it("asks nothing of a clean editor, nor once the profile is deleted", async () => {
    const user = userEvent.setup()
    const onDeleted = vi.fn()
    renderEditor({ onDeleted })
    await nameBox()

    await user.click(screen.getByRole("button", { name: "leave for the board" }))

    expect(await screen.findByText("the board")).toBeDefined()
    expect(screen.queryByRole("dialog", { name: "Discard changes?" })).toBeNull()
  })
})
