// @vitest-environment jsdom

/**
 * The create dialog against a stubbed daemon.
 *
 * Three fields and one request, so what is worth pinning down is the request:
 * a blank pin reaches the daemon as no model at all, typed free text reaches
 * it untouched, a model naming no agent CLI is refused before it is sent, and
 * the screen is handed the new profile to land on. The pin picker's own
 * behaviour is `pin-picker.test.tsx`'s; the two rows here are the ones that
 * only make sense inside a form.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { ModelDto, ProfileDto } from "@/api"
import { aModel, anEffort, aProfile } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { CreateProfileDialog } from "./create-profile-dialog"

const CREATED: ProfileDto = aProfile({ id: "01JPROF00000000000000NEW", name: "custom" })

/** A slice of the daemon's curated catalog, two agents wide. */
const CATALOG: ModelDto[] = [
  aModel({
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
    efforts: [
      anEffort({ id: "low" }),
      anEffort({ id: "medium" }),
      anEffort({ id: "high", default: true }),
      anEffort({ id: "xhigh" }),
      anEffort({ id: "max" }),
    ],
  }),
  aModel({
    id: "codex:gpt-5.6-terra",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
    efforts: [
      anEffort({ id: "low" }),
      anEffort({ id: "medium", default: true }),
      anEffort({ id: "high" }),
      anEffort({ id: "xhigh" }),
    ],
  }),
]

interface Recorded {
  method: string
  path: string
  body: Record<string, unknown> | null
}

let requests: Recorded[] = []

/** The one write this dialog makes, if it made it. */
function created(): Recorded | undefined {
  return requests.find((one) => one.method === "POST" && one.path === "/v1/profiles")
}

function stubDaemon({
  models = CATALOG,
  nameTaken = false,
}: {
  models?: ModelDto[] | "error"
  nameTaken?: boolean
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
    if (pathname === "/v1/profiles" && request.method === "POST") {
      return nameTaken
        ? errorResponse(409, "profile_name_taken", "a profile with that name exists")
        : jsonResponse({ ...CREATED, ...body }, 201)
    }
    return new Response("not stubbed", { status: 404 })
  })
}

function renderDialog({
  onOpenChange = () => {},
  onCreated = () => {},
}: {
  onOpenChange?: (open: boolean) => void
  onCreated?: (profile: ProfileDto) => void
} = {}) {
  return renderScreen(
    <CreateProfileDialog open onOpenChange={onOpenChange} onCreated={onCreated} />,
  )
}

/** Opens the picker, and answers the catalog inside it. */
async function openPin(user: ReturnType<typeof userEvent.setup>): Promise<HTMLElement> {
  await user.click(await screen.findByRole("button", { name: "Runs on" }))
  return await screen.findByRole("listbox", { name: "Models" })
}

/** Types into the picker's search box, which is also its free-text field. */
async function searchPin(user: ReturnType<typeof userEvent.setup>, text: string) {
  await user.type(screen.getByRole("combobox", { name: "Runs on" }), text)
}

/** Shuts the picker again, the way the dialog behind it must survive. */
async function closePin(user: ReturnType<typeof userEvent.setup>) {
  await user.keyboard("{Escape}")
  await waitFor(() => {
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  })
}

beforeEach(() => {
  requests = []
  stubDaemon()
})

describe("creating a profile", () => {
  it("sends a name, a role and no pin, and hands the screen what came back", async () => {
    const user = userEvent.setup()
    const onCreated = vi.fn()
    const onOpenChange = vi.fn()
    renderDialog({ onCreated, onOpenChange })

    await user.type(screen.getByLabelText("Name"), "rust-reviewer")
    await user.click(screen.getByLabelText("Role"))
    await user.click(await screen.findByRole("option", { name: "Reviewer" }))
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => {
      expect(created()).toBeDefined()
    })
    // No prompt is typed here: the profile is created on its role's own, and
    // the editor it lands in is where they are rewritten.
    expect(created()?.body).toEqual({
      name: "rust-reviewer",
      role: "reviewer",
      model: null,
      effort: null,
      system_prompt: null,
    })
    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ id: CREATED.id }))
    expect(screen.queryByRole("textbox", { name: "System prompt" })).toBeNull()
  })

  it("pins a catalog model at one of its efforts, for the role that was picked", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Name"), "rust-reviewer")
    await user.click(screen.getByLabelText("Role"))
    await user.click(await screen.findByRole("option", { name: "Reviewer" }))
    const options = await openPin(user)
    await user.click(within(options).getByText("codex:gpt-5.6-terra"))
    // The codex entry's own list, not the claude one beside it in the catalog.
    expect(screen.queryByRole("radio", { name: "max" })).toBeNull()
    await user.click(await screen.findByRole("radio", { name: "high" }))
    await closePin(user)
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => expect(created()).toBeDefined())
    expect(created()?.body).toEqual({
      name: "rust-reviewer",
      role: "reviewer",
      model: "codex:gpt-5.6-terra",
      effort: "high",
      system_prompt: null,
    })
  })

  it("sends typed free text as the model, matched by nothing in the catalog", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Name"), "custom")
    await openPin(user)
    await searchPin(user, "opencode:my-weird-model")
    await user.click(screen.getByText(/^Other — run/))
    await closePin(user)
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => expect(created()).toBeDefined())
    expect(created()?.body?.model).toBe("opencode:my-weird-model")
  })

  it("refuses a model that names no agent CLI, before the daemon is asked", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Name"), "custom")
    await openPin(user)
    await searchPin(user, "claude-opus-5")
    await user.click(screen.getByText(/^Other — run/))
    await closePin(user)
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    expect(await screen.findByText(/claude_code:claude-opus-5/)).toBeDefined()
    expect(created()).toBeUndefined()
  })

  it("still takes a typed model when the catalog cannot be fetched", async () => {
    const user = userEvent.setup()
    stubDaemon({ models: "error" })
    renderDialog()

    await openPin(user)
    await searchPin(user, "codex:still-typable")
    await user.click(screen.getByText(/^Other — run/))
    await closePin(user)
    await user.type(screen.getByLabelText("Name"), "offline")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    await waitFor(() => expect(created()).toBeDefined())
    expect(created()?.body?.model).toBe("codex:still-typable")
  })

  it("puts a name clash on the name field and stays open", async () => {
    const user = userEvent.setup()
    stubDaemon({ nameTaken: true })
    const onOpenChange = vi.fn()
    renderDialog({ onOpenChange })

    await user.type(screen.getByLabelText("Name"), "engineer")
    await user.click(screen.getByRole("button", { name: "Create profile" }))

    expect(await screen.findByText('A profile named "engineer" already exists.')).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalled()
  })

  it("asks for a name before asking the daemon", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.click(screen.getByRole("button", { name: "Create profile" }))

    expect(await screen.findByText("Give the profile a name.")).toBeDefined()
    expect(created()).toBeUndefined()
  })
})
