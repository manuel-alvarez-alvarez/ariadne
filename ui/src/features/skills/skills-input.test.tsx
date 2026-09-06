// @vitest-environment jsdom

/**
 * The control an agent's skills are typed into.
 *
 * Free text against the catalog rather than a closed select, because the
 * catalog is the user's to extend and a task may name a skill written a
 * minute ago. So the catalog is offered, and a name it does not hold is
 * *said* rather than refused — the daemon is what refuses one.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useForm } from "react-hook-form"
import { describe, expect, it } from "vitest"

import { renderScreen } from "@/test/harness"
import { SkillsInput } from "./skills-input"

const CATALOG = ["coding", "testing", "code-review"]

function Harness({
  initial = "",
  suggestions = CATALOG,
}: {
  initial?: string
  suggestions?: string[]
}) {
  const { control } = useForm<{ skills: string }>({ defaultValues: { skills: initial } })
  return (
    <SkillsInput
      control={control}
      name="skills"
      id="agent-skills"
      ariaLabel="Skills"
      suggestions={suggestions}
    />
  )
}

function input(): HTMLInputElement {
  return screen.getByRole("combobox", { name: "Skills" }) as HTMLInputElement
}

describe("SkillsInput", () => {
  it("offers every skill in the catalog as a completion of its own box", () => {
    renderScreen(<Harness />)

    const list = document.getElementById(input().getAttribute("list") ?? "")
    expect([...(list?.children ?? [])].map((option) => option.getAttribute("value"))).toEqual(
      CATALOG,
    )
  })

  it("says nothing while every name is one the catalog holds", async () => {
    const user = userEvent.setup()
    renderScreen(<Harness />)

    await user.type(input(), "coding, testing")

    expect(screen.queryByText(/No skill named/)).toBeNull()
  })

  it("names the ones nothing answers to, without taking them away", async () => {
    const user = userEvent.setup()
    renderScreen(<Harness />)

    await user.type(input(), "coding, made-up, also-made-up")

    expect(screen.getByText("No skill named “made-up”, “also-made-up”.")).toBeDefined()
    // Said, not enforced: what was typed is still what will be sent.
    expect(input().value).toBe("coding, made-up, also-made-up")
  })

  it("says nothing while the catalog has not arrived", async () => {
    const user = userEvent.setup()
    renderScreen(<Harness suggestions={[]} />)

    await user.type(input(), "made-up")

    expect(screen.queryByText(/No skill named/)).toBeNull()
  })
})
