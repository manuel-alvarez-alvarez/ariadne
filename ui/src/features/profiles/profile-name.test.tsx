// @vitest-environment jsdom

/**
 * A profile mention is a way to the profile.
 *
 * The name used to sit on a copy button, which put the ULID on the clipboard
 * and left the mention the one entity reference in the app that went nowhere.
 * What is worth pinning down is the two halves of the link: a known profile is
 * named and points at `?expand=<id>`, and one the list has never heard of —
 * deleted, or still loading — is still a link, wearing its id. The hover
 * carries both, name first: it answers a mention the column cut short.
 *
 * The names are seeded straight into the query cache: what this renders from
 * is `profilesQueryOptions`, and the daemon behind it is `queries.ts`'s story.
 */

import { screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { type ProfileDto, qk } from "@/api"
import { paths } from "@/routes/paths"
import { aProfile } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { ProfileName } from "./profile-name"

const PROFILE: ProfileDto = aProfile({
  id: "01JPROF000000000000000ENG",
  name: "Builder",
})

function mount(profileId: string) {
  renderScreen(<ProfileName profileId={profileId} />, {
    seed: (client) => client.setQueryData(qk.profiles.list({}), [PROFILE]),
  })
}

it("shows the name, linking to the profile's row", () => {
  mount(PROFILE.id)

  const link = screen.getByRole("link", { name: "Builder" })
  expect(link.getAttribute("href")).toBe(paths.profile(PROFILE.id))
  // The name leads the hover: a mention cut to `Buil…` in a table cell is
  // hovered for the half that was cut off, not for a ULID. The id follows it,
  // off the clipboard but not out of reach.
  expect(link.getAttribute("title")).toBe(`Builder · ${PROFILE.id}`)
  expect(screen.queryByRole("button", { name: /copy/i })).toBeNull()
})

it("falls back to the id of a profile the list does not have", () => {
  mount("01JPROF000000000000000GON")

  const link = screen.getByRole("link", { name: "01JPROF000000000000000GON" })
  expect(link.getAttribute("href")).toBe(paths.profile("01JPROF000000000000000GON"))
})
