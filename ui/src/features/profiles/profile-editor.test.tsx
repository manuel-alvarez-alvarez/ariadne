// @vitest-environment jsdom

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { beforeEach, describe, expect, it } from "vitest"

import type { ProfileDto } from "@/api"
import { paths } from "@/routes/paths"
import { aProfile } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { ProfileEditor } from "./profile-editor"

const profile: ProfileDto = aProfile({
  id: "01JPROF000000000000000ENG",
  name: "Builder",
  system_prompt: "Stored system prompt.",
  system_prompt_is_default: false,
})
let requests: { method: string; path: string; body: unknown }[] = []

function renderEditor() {
  const router = createMemoryRouter(
    [
      {
        path: paths.profiles(),
        element: <ProfileEditor profile={profile} onBack={() => {}} onDeleted={() => {}} />,
      },
    ],
    { initialEntries: [paths.profile(profile.id)] },
  )
  return renderScreen(<RouterProvider router={router} />, { route: null })
}

function stubDaemon() {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const path = new URL(request.url).pathname
    const raw = await request.text()
    requests.push({ method: request.method, path, body: raw ? JSON.parse(raw) : null })
    if (path === "/v1/models") return jsonResponse([])
    if (path === `/v1/profiles/${profile.id}` && request.method === "PUT") {
      return jsonResponse({ ...profile, ...(raw ? JSON.parse(raw) : {}) })
    }
    if (path === `/v1/profiles/${profile.id}/system-prompt/reset`) {
      return jsonResponse({
        ...profile,
        system_prompt: "Default system prompt.",
        system_prompt_is_default: true,
      })
    }
    return new Response("not stubbed", { status: 404 })
  })
}

function profileWrites() {
  return requests.filter((request) => request.method !== "GET")
}

describe("ProfileEditor", () => {
  beforeEach(() => {
    requests = []
  })

  it("shows only the system prompt and never requests lifecycle prompts", async () => {
    stubDaemon()
    renderEditor()

    expect(
      ((await screen.findByRole("textbox", { name: "System prompt" })) as HTMLTextAreaElement)
        .value,
    ).toBe("Stored system prompt.")
    expect(screen.queryByText(/Engineer briefing|Changes requested|Planner briefing/)).toBeNull()
    expect(requests.map((request) => request.path)).not.toContain(
      `/v1/profiles/${profile.id}/prompts`,
    )
  })

  it("saves system prompt and profile field edits in one profile request", async () => {
    stubDaemon()
    const user = userEvent.setup()
    renderEditor()

    await user.type(await screen.findByLabelText("Name"), "-v2")
    await user.type(screen.getByRole("textbox", { name: "System prompt" }), " More.")
    await user.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(profileWrites()).toHaveLength(1))
    expect(profileWrites()[0]).toMatchObject({
      method: "PUT",
      path: `/v1/profiles/${profile.id}`,
      body: { name: "Builder-v2", system_prompt: "Stored system prompt. More." },
    })
  })

  it("restores the system prompt immediately and does not save it again with another edit", async () => {
    stubDaemon()
    const user = userEvent.setup()
    renderEditor()

    await user.click(
      await screen.findByRole("button", { name: "Restore System prompt to default" }),
    )
    const dialog = await screen.findByRole("dialog", {
      name: "Restore system prompt to its default?",
    })
    await user.click(within(dialog).getByRole("button", { name: "Restore default" }))
    await screen.findByDisplayValue("Default system prompt.")
    await user.type(screen.getByLabelText("Name"), "-v2")
    await user.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(profileWrites()).toHaveLength(2))
    expect(profileWrites()[0]).toMatchObject({
      method: "POST",
      path: `/v1/profiles/${profile.id}/system-prompt/reset`,
    })
    expect(profileWrites()[1]).toMatchObject({ method: "PUT", body: { name: "Builder-v2" } })
  })
})
