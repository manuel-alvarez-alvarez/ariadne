// @vitest-environment jsdom

/**
 * The picker as the goal and task forms use it: the whole catalog, no agent
 * control anywhere near it.
 *
 * That is the half the profile form's own tests cannot cover — there the
 * options are scoped by an agent select, and the agent is a thing the user
 * picks. Here the model *is* the choice and the agent follows from it, so what
 * has to hold is that every agent's models are on offer at once, that a pick
 * yields the id itself, and that the field says which CLI the pick commits to
 * — including for a model the catalog has never heard of, which the daemon
 * places on its own.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it } from "vitest"

import type { ModelDto } from "@/api"
import { renderScreen } from "@/test/harness"

import { ModelPicker } from "./model-picker"

/** A slice of the daemon's catalog, one entry per agent CLI. */
const CATALOG: ModelDto[] = [
  { id: "claude-opus-5", agent_kind: "claude_code", description: "Opus tier: deep analysis" },
  { id: "gpt-5.3-codex", agent_kind: "codex", description: "Frontier reasoning: agentic loops" },
  { id: "zai-coding-plan/glm-4.6", agent_kind: "opencode", description: null },
]

/**
 * The picker holding its own value, the way a form field holds it.
 *
 * `catalog: false` is the endpoint that never answered, which has to leave a
 * plain text field behind rather than a combobox with nothing in it.
 */
function renderPicker({ catalog = true }: { catalog?: boolean } = {}) {
  function Host() {
    const [value, setValue] = useState("")
    return (
      <ModelPicker
        value={value}
        onChange={setValue}
        models={catalog ? CATALOG : undefined}
        caption
        label="Model"
      />
    )
  }
  renderScreen(<Host />, { route: null })
  return screen.getByRole("combobox", { name: "Model" }) as HTMLInputElement
}

/** The catalog popup, which lives in a portal outside the field. */
async function listbox(): Promise<HTMLElement> {
  return await screen.findByRole("listbox", { name: "Models" })
}

it("offers every agent kind's models at once, under a heading each", async () => {
  const user = userEvent.setup()
  const box = renderPicker()

  await user.click(box)
  const options = await listbox()

  expect(within(options).getByText("Claude Code")).toBeDefined()
  expect(within(options).getByText("Codex")).toBeDefined()
  expect(within(options).getByText("OpenCode")).toBeDefined()
  for (const model of CATALOG) {
    expect(within(options).getByText(model.id)).toBeDefined()
  }
})

it("puts the picked id in the field and names the agent it runs on", async () => {
  const user = userEvent.setup()
  const box = renderPicker()

  await user.click(box)
  await user.click(within(await listbox()).getByText("gpt-5.3-codex"))

  // The model alone is the value: nothing here sends an agent kind.
  expect(box.value).toBe("gpt-5.3-codex")
  expect(await screen.findByText("Runs on Codex.")).toBeDefined()
  await waitFor(() => {
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  })
})

it("keeps free text and leaves its agent to the daemon", async () => {
  const user = userEvent.setup()
  const box = renderPicker()

  await user.type(box, "some-unlisted-model")

  expect(box.value).toBe("some-unlisted-model")
  expect(await screen.findByText("Agent CLI derived by the daemon.")).toBeDefined()
})

it("says nothing about an empty field, which is the profile's own model", () => {
  renderPicker()

  expect(screen.queryByText(/Runs on/)).toBeNull()
  expect(screen.queryByText("Agent CLI derived by the daemon.")).toBeNull()
})

it("stays a plain text field when the catalog never arrived", async () => {
  const user = userEvent.setup()
  const box = renderPicker({ catalog: false })

  await user.click(box)
  await user.type(box, "claude-opus-5")

  expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  expect(box.value).toBe("claude-opus-5")
})
