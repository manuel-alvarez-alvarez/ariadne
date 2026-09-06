// @vitest-environment jsdom

/**
 * A skill name, wherever an agent is mentioned.
 *
 * An agent has no name of its own, so a skill name is what a reader has to go
 * on, and the one thing in such a mention that can be followed. The summaries
 * behind it come from the one skills list every screen shares, so a name shown
 * before that list has arrived — or one no skill answers to any more — still
 * links rather than breaking the line it sits in.
 */

import { screen } from "@testing-library/react"
import { beforeEach, describe, expect, it } from "vitest"

import { paths } from "@/routes/paths"
import { aSkill } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SkillName } from "./skill-name"

const CODING = aSkill({ name: "coding", summary: "Implement a task from its specification." })

beforeEach(() => {
  daemonFetch.mockImplementation(async () => jsonResponse([CODING]))
})

describe("SkillName", () => {
  it("links to the skill on the skills screen", async () => {
    renderScreen(<SkillName name="coding" />)

    const link = await screen.findByRole("link", { name: "coding" })
    expect(link.getAttribute("href")).toBe(paths.skill("coding"))
  })

  it("reads the summaries from the one list every screen shares", async () => {
    renderScreen(
      <>
        <SkillName name="coding" />
        <SkillName name="code-review" />
        <SkillName name="coding" />
      </>,
    )

    await screen.findAllByRole("link", { name: "coding" })
    // Three mentions, one request: the key is the skills screen's own, so
    // whichever screen loads it first serves the rest.
    expect(daemonFetch.mock.calls.length).toBe(1)
  })

  it("falls back to the name for a skill nothing answers to", async () => {
    renderScreen(<SkillName name="gone-since" />)

    const link = await screen.findByRole("link", { name: "gone-since" })
    expect(link.getAttribute("href")).toBe(paths.skill("gone-since"))
  })
})
