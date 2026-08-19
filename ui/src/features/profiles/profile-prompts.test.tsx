// @vitest-environment jsdom

/**
 * The prompt editors, against a stubbed daemon.
 *
 * Rendered rather than unit-tested because everything worth checking is a round
 * trip: which prompts a role gets is the daemon's answer and not a map in the
 * UI, a save goes to one endpoint per prompt, and a restore has to put the
 * daemon's default into the box — including over an unsaved draft, which is the
 * case a cache patch alone would miss.
 *
 * No default prompt text exists on this side, so the stub is the only place any
 * appears; a test that expected the UI to know a default would be testing the
 * bug this feature must not have.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { ProfileDto, ProfilePromptDto } from "@/api"

import { ProfilePrompts } from "./profile-prompts"

/** Hoisted for the reason `profiles-page.test.tsx` gives: the client takes its
 * `fetch` when `@/api` is imported, so a later stub is one it never sees. */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const STAMP = "2026-01-01T00:00:00Z"

const ENGINEER: ProfileDto = {
  id: "01JPROF000000000000000ENG",
  name: "Builder",
  role: "engineer",
  agent_kind: "claude_code",
  model: null,
  system_prompt: "Stored system prompt.",
  created_at: STAMP,
  updated_at: STAMP,
}

const PLANNER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000PLN",
  name: "Mapper",
  role: "planner",
}

const REVIEWER: ProfileDto = {
  ...ENGINEER,
  id: "01JPROF000000000000000REV",
  name: "Critic",
  role: "reviewer",
}

/** What the daemon holds, per role — the kinds it answers `GET .../prompts` with. */
const STORED: Record<string, ProfilePromptDto[]> = {
  engineer: [
    { kind: "engineer_briefing", content: "Stored engineer briefing.", updated_at: STAMP },
    { kind: "changes_requested", content: "Stored changes requested.", updated_at: STAMP },
    { kind: "merge_instructions", content: "Stored merge instructions.", updated_at: STAMP },
  ],
  planner: [{ kind: "planner_briefing", content: "Stored planner briefing.", updated_at: STAMP }],
  reviewer: [
    { kind: "reviewer_briefing", content: "Stored reviewer briefing.", updated_at: STAMP },
    { kind: "reviewer_resume", content: "Stored reviewer resume.", updated_at: STAMP },
  ],
}

/** What a reset answers. Only the stub knows these; the UI never does. */
const DEFAULT_SYSTEM_PROMPT = "Default system prompt for {role}."
const DEFAULT_PROMPTS: Record<string, string> = {
  engineer_briefing: "Default engineer briefing for {task}.",
  changes_requested: "Default changes requested for {task}.",
  merge_instructions: "Default merge instructions for {task}.",
  planner_briefing: "Default planner briefing for {goal}.",
  reviewer_briefing: "Default reviewer briefing for {task}.",
  reviewer_resume: "Default reviewer resume for {task}.",
}

interface Recorded {
  method: string
  path: string
  body: { content?: string; system_prompt?: string } | null
}

let requests: Recorded[] = []

/** The last request that went to `path` with `method`, or undefined. */
function lastRequest(method: string, path: string): Recorded | undefined {
  return requests.filter((one) => one.method === method && one.path === path).at(-1)
}

/**
 * The daemon's profile-prompt endpoints, routed by path the way it routes them.
 * `refusal`, when given, is the message it answers a briefing save with — the
 * daemon refuses a `{placeholder}` its kind cannot fill in.
 */
function stubDaemon(profile: ProfileDto, refusal?: string) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const { pathname } = new URL(request.url)
    const raw = await request.text()
    const body = raw.length > 0 ? JSON.parse(raw) : null
    requests.push({ method: request.method, path: pathname, body })

    const prompts = STORED[profile.role] ?? []
    const base = `/v1/profiles/${profile.id}`
    const answer = (payload: unknown) =>
      new Response(JSON.stringify(payload), {
        status: 200,
        headers: { "content-type": "application/json" },
      })

    if (pathname === `${base}/prompts`) return answer(prompts)
    if (pathname === `${base}/system-prompt/reset`) {
      return answer({ ...profile, system_prompt: DEFAULT_SYSTEM_PROMPT })
    }
    if (pathname === base) return answer({ ...profile, ...body })

    const reset = pathname.match(/\/prompts\/([a-z_]+)\/reset$/)?.[1]
    if (reset) return answer({ kind: reset, content: DEFAULT_PROMPTS[reset], updated_at: STAMP })

    const written = pathname.match(/\/prompts\/([a-z_]+)$/)?.[1]
    if (written) {
      if (refusal && request.method === "PUT") {
        return new Response(
          JSON.stringify({ error: { code: "invalid_request", message: refusal } }),
          { status: 400, headers: { "content-type": "application/json" } },
        )
      }
      return answer({ kind: written, content: body?.content, updated_at: STAMP })
    }

    return new Response("not stubbed", { status: 404 })
  })
}

function renderPrompts(profile: ProfileDto) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  return render(
    <QueryClientProvider client={queryClient}>
      <ProfilePrompts profile={profile} />
    </QueryClientProvider>,
  )
}

/** The editor for one prompt, once its content has arrived. */
async function editor(label: string): Promise<HTMLTextAreaElement> {
  return (await screen.findByLabelText(label)) as HTMLTextAreaElement
}

beforeEach(() => {
  requests = []
  daemonFetch.mockReset()
  stubDaemon(ENGINEER)
})

afterEach(cleanup)

describe("ProfilePrompts", () => {
  it("shows the system prompt and every briefing the daemon lists for the role", async () => {
    renderPrompts(ENGINEER)

    expect((await editor("System prompt")).value).toBe("Stored system prompt.")
    expect((await editor("Engineer briefing")).value).toBe("Stored engineer briefing.")
    expect((await editor("Changes requested")).value).toBe("Stored changes requested.")
    expect((await editor("Merge instructions")).value).toBe("Stored merge instructions.")
    // A kind of another role is a kind the daemon never sent.
    expect(screen.queryByLabelText("Reviewer briefing")).toBeNull()
  })

  it("shows a planner its one briefing and none of the engineer's", async () => {
    stubDaemon(PLANNER)
    renderPrompts(PLANNER)

    expect((await editor("Planner briefing")).value).toBe("Stored planner briefing.")
    expect(screen.queryByLabelText("Engineer briefing")).toBeNull()
    expect(screen.queryByLabelText("Merge instructions")).toBeNull()
  })

  it("shows a reviewer both of its briefings and none of the other roles'", async () => {
    stubDaemon(REVIEWER)
    renderPrompts(REVIEWER)

    expect((await editor("System prompt")).value).toBe("Stored system prompt.")
    expect((await editor("Reviewer briefing")).value).toBe("Stored reviewer briefing.")
    expect((await editor("Reviewer resume")).value).toBe("Stored reviewer resume.")
    expect(screen.queryByLabelText("Planner briefing")).toBeNull()
    expect(screen.queryByLabelText("Engineer briefing")).toBeNull()
    expect(screen.queryByLabelText("Changes requested")).toBeNull()
    expect(screen.queryByLabelText("Merge instructions")).toBeNull()
  })

  it("saves and restores a reviewer's briefings on their own endpoints", async () => {
    const user = userEvent.setup()
    stubDaemon(REVIEWER)
    renderPrompts(REVIEWER)
    const box = await editor("Reviewer resume")

    await user.clear(box)
    await user.type(box, "Look again.")
    await user.click(screen.getByRole("button", { name: "Save Reviewer resume" }))

    await waitFor(() => {
      expect(
        lastRequest("PUT", `/v1/profiles/${REVIEWER.id}/prompts/reviewer_resume`),
      ).toBeDefined()
    })
    expect(lastRequest("PUT", `/v1/profiles/${REVIEWER.id}/prompts/reviewer_resume`)?.body).toEqual(
      {
        content: "Look again.",
      },
    )

    await user.click(screen.getByRole("button", { name: "Restore Reviewer resume to default" }))
    await screen.findByText("Restore the default reviewer resume?")
    await user.click(screen.getByRole("button", { name: "Restore default" }))

    await waitFor(() => {
      expect(box.value).toBe(DEFAULT_PROMPTS.reviewer_resume)
    })
  })

  it("saves one briefing on its own endpoint, broken template and all", async () => {
    const user = userEvent.setup()
    renderPrompts(ENGINEER)
    const box = await editor("Changes requested")

    await user.clear(box)
    await user.type(box, "No placeholder here at all.")
    await user.click(screen.getByRole("button", { name: "Save Changes requested" }))

    await waitFor(() => {
      expect(
        lastRequest("PUT", `/v1/profiles/${ENGINEER.id}/prompts/changes_requested`),
      ).toBeDefined()
    })
    expect(
      lastRequest("PUT", `/v1/profiles/${ENGINEER.id}/prompts/changes_requested`)?.body,
    ).toEqual({ content: "No placeholder here at all." })
    // The other prompts are untouched: one endpoint per prompt, one save each.
    expect(
      lastRequest("PUT", `/v1/profiles/${ENGINEER.id}/prompts/engineer_briefing`),
    ).toBeUndefined()
  })

  it("shows the daemon's refusal when a briefing names a placeholder it cannot fill in", async () => {
    const user = userEvent.setup()
    const refusal =
      "the engineer_briefing template has no value for placeholder {task_titel}; " +
      "the ones it can use are {task_title}, {task_description}, {goal_title}, " +
      "{worktree_path}, {branch}, {base_branch}, {repo_path}, {dependencies}"
    stubDaemon(ENGINEER, refusal)
    renderPrompts(ENGINEER)
    const box = await editor("Engineer briefing")

    await user.clear(box)
    // `{{` is how userEvent types a literal brace; the box gets "# {task_titel}".
    await user.type(box, "# {{task_titel}")
    await user.click(screen.getByRole("button", { name: "Save Engineer briefing" }))

    // The daemon's sentence, verbatim: the offending token and what was allowed
    // instead are the whole point of showing it.
    expect(await screen.findByText(new RegExp(escapeRegExp(refusal)))).toBeDefined()
    expect(await screen.findByText("Could not save the engineer briefing")).toBeDefined()
    // The draft survives, so the typo can be fixed rather than retyped.
    expect(box.value).toBe("# {task_titel}")
  })

  it("saves the system prompt through the profile itself, leaving its other fields alone", async () => {
    const user = userEvent.setup()
    renderPrompts(ENGINEER)
    const box = await editor("System prompt")

    await user.clear(box)
    await user.type(box, "Rewritten.")
    await user.click(screen.getByRole("button", { name: "Save System prompt" }))

    await waitFor(() => {
      expect(lastRequest("PUT", `/v1/profiles/${ENGINEER.id}`)).toBeDefined()
    })
    expect(lastRequest("PUT", `/v1/profiles/${ENGINEER.id}`)?.body).toEqual({
      system_prompt: "Rewritten.",
    })
  })

  it("does not offer to save a prompt nobody has touched", async () => {
    renderPrompts(ENGINEER)
    await editor("Engineer briefing")

    expect(
      screen.getByRole("button", { name: "Save Engineer briefing" }).hasAttribute("disabled"),
    ).toBe(true)
  })

  it("asks before restoring, and puts the daemon's default in the box", async () => {
    const user = userEvent.setup()
    renderPrompts(ENGINEER)
    const box = await editor("Engineer briefing")

    await user.click(screen.getByRole("button", { name: "Restore Engineer briefing to default" }))

    // Nothing has moved yet: the confirmation is the whole point.
    expect(await screen.findByText("Restore the default engineer briefing?")).toBeDefined()
    expect(box.value).toBe("Stored engineer briefing.")
    expect(requests.some((one) => one.method === "POST")).toBe(false)

    await user.click(screen.getByRole("button", { name: "Restore default" }))

    await waitFor(() => {
      expect(box.value).toBe(DEFAULT_PROMPTS.engineer_briefing)
    })
    expect(
      lastRequest("POST", `/v1/profiles/${ENGINEER.id}/prompts/engineer_briefing/reset`),
    ).toBeDefined()
  })

  it("lets the confirmation be dismissed, and changes nothing when it is", async () => {
    const user = userEvent.setup()
    renderPrompts(ENGINEER)
    const box = await editor("Merge instructions")

    await user.click(screen.getByRole("button", { name: "Restore Merge instructions to default" }))
    await screen.findByText("Restore the default merge instructions?")
    await user.click(screen.getByRole("button", { name: "Cancel" }))

    await waitFor(() => {
      expect(screen.queryByText("Restore the default merge instructions?")).toBeNull()
    })
    expect(box.value).toBe("Stored merge instructions.")
    expect(requests.some((one) => one.method === "POST")).toBe(false)
  })

  it("restores over an unsaved draft", async () => {
    const user = userEvent.setup()
    renderPrompts(ENGINEER)
    const box = await editor("Changes requested")

    await user.clear(box)
    await user.type(box, "Half-written.")
    await user.click(screen.getByRole("button", { name: "Restore Changes requested to default" }))
    await screen.findByText("Restore the default changes requested?")
    await user.click(screen.getByRole("button", { name: "Restore default" }))

    await waitFor(() => {
      expect(box.value).toBe(DEFAULT_PROMPTS.changes_requested)
    })
  })

  it("restores the system prompt from the profile the daemon answers with", async () => {
    const user = userEvent.setup()
    renderPrompts(ENGINEER)
    const box = await editor("System prompt")

    await user.click(screen.getByRole("button", { name: "Restore System prompt to default" }))
    await screen.findByText("Restore the default system prompt?")
    await user.click(screen.getByRole("button", { name: "Restore default" }))

    await waitFor(() => {
      expect(box.value).toBe(DEFAULT_SYSTEM_PROMPT)
    })
    expect(lastRequest("POST", `/v1/profiles/${ENGINEER.id}/system-prompt/reset`)).toBeDefined()
  })
})

/** The refusal is matched as a whole sentence; nothing in it is a pattern. */
function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}
