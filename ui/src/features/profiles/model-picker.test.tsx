// @vitest-environment jsdom

/**
 * The picker as the forms that assign an agent use it: scoped to the CLI the
 * select beside it names, and shut while it names none.
 *
 * The model is the narrower half of the choice, so the field is only worth
 * anything once there is an agent for it to narrow: until then it is disabled
 * and its catalog stays down. Once there is one, the suggestions are that
 * CLI's models and nothing else — a codex box never offers a claude model —
 * and a pick yields the id itself, free text included, since the daemon hands
 * whatever is typed to the CLI as-is.
 *
 * The unscoped mode is the profile form's "Auto-resolve", where there is no
 * CLI yet to offer the models of: the catalog whole, with a heading per agent.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it } from "vitest"

import type { AgentKind, ModelDto } from "@/api"
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
function renderPicker({
  catalog = true,
  agentKind,
  disabled = false,
}: {
  catalog?: boolean
  agentKind?: AgentKind
  disabled?: boolean
} = {}) {
  function Host() {
    const [value, setValue] = useState("")
    return (
      <ModelPicker
        value={value}
        onChange={setValue}
        models={catalog ? CATALOG : undefined}
        agentKind={agentKind}
        disabled={disabled}
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

it("stays shut and untypeable while no agent is pinned", async () => {
  const user = userEvent.setup()
  const box = renderPicker({ disabled: true })

  expect(box.disabled).toBe(true)

  await user.click(box)
  await user.type(box, "claude-opus-5")

  expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  expect(box.value).toBe("")
})

it("offers the pinned agent's models and no others", async () => {
  const user = userEvent.setup()
  const box = renderPicker({ agentKind: "codex" })

  await user.click(box)
  const options = await listbox()

  expect(within(options).getByText("gpt-5.3-codex")).toBeDefined()
  expect(within(options).queryByText("claude-opus-5")).toBeNull()
  expect(within(options).queryByText("zai-coding-plan/glm-4.6")).toBeNull()
  // Scoped, so the agent is not worth a heading: it is the select's to say.
  expect(within(options).queryByText("Codex")).toBeNull()
})

it("puts the picked id in the field", async () => {
  const user = userEvent.setup()
  const box = renderPicker({ agentKind: "codex" })

  await user.click(box)
  await user.click(within(await listbox()).getByText("gpt-5.3-codex"))

  expect(box.value).toBe("gpt-5.3-codex")
  await waitFor(() => {
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  })
})

it("keeps free text, which is handed to the CLI as typed", async () => {
  const user = userEvent.setup()
  const box = renderPicker({ agentKind: "codex" })

  await user.type(box, "some-unlisted-model")

  expect(box.value).toBe("some-unlisted-model")
})

it("offers every agent kind's models at once when it is scoped to none", async () => {
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

it("stays a plain text field when the catalog never arrived", async () => {
  const user = userEvent.setup()
  const box = renderPicker({ catalog: false, agentKind: "claude_code" })

  await user.click(box)
  await user.type(box, "claude-opus-5")

  expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  expect(box.value).toBe("claude-opus-5")
})
