// @vitest-environment jsdom

/**
 * A profile mention is a way to the profile.
 *
 * The name used to sit on a copy button, which put the ULID on the clipboard
 * and left the mention the one entity reference in the app that went nowhere.
 * What is worth pinning down is the two halves of the link: a known profile is
 * named and points at `?expand=<id>`, and one the list has never heard of —
 * deleted, or still loading — is still a link, wearing its id.
 *
 * The names are seeded straight into the query cache: what this renders from
 * is `profilesQueryOptions`, and the daemon behind it is `queries.ts`'s story.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, expect, it } from "vitest"

import { type ProfileDto, qk } from "@/api"
import { paths } from "@/routes/paths"

import { ProfileName } from "./profile-name"

afterEach(cleanup)

const PROFILE: ProfileDto = {
  id: "01JPROF000000000000000ENG",
  name: "Builder",
  role: "engineer",
  agent_kind: "claude_code",
  model: null,
  system_prompt: "",
  system_prompt_is_default: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

function mount(profileId: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  client.setQueryData(qk.profiles.list({}), [PROFILE])
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ProfileName profileId={profileId} />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

it("shows the name, linking to the profile's row", () => {
  mount(PROFILE.id)

  const link = screen.getByRole("link", { name: "Builder" })
  expect(link.getAttribute("href")).toBe(paths.profile(PROFILE.id))
  // The id is off the clipboard, but not out of reach.
  expect(link.getAttribute("title")).toBe(PROFILE.id)
  expect(screen.queryByRole("button", { name: /copy/i })).toBeNull()
})

it("falls back to the id of a profile the list does not have", () => {
  mount("01JPROF000000000000000GON")

  const link = screen.getByRole("link", { name: "01JPROF000000000000000GON" })
  expect(link.getAttribute("href")).toBe(paths.profile("01JPROF000000000000000GON"))
})
