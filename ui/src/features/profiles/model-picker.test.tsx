// @vitest-environment jsdom

/**
 * The picker as every form that assigns a model uses it: one field, holding the
 * whole choice.
 *
 * The value is a qualified id — the agent CLI and, after a `:`, the model of it
 * — so there is nothing beside the field to scope it and nothing to wait for
 * before it is worth opening: the catalog is offered whole, under a heading per
 * agent CLI, each group led by that CLI on its own default model. A pick yields
 * the id itself, and free text is kept, since the daemon hands whatever is
 * typed to the CLI it names.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it } from "vitest"

import type { ModelDto } from "@/api"
import { renderScreen } from "@/test/harness"

import { ModelPicker } from "./model-picker"

/**
 * A slice of the daemon's catalog: each agent CLI on its own, and one model of
 * it — deliberately not in agent order, since the picker groups it itself.
 */
const CATALOG: ModelDto[] = [
  {
    id: "codex:gpt-5.3-codex",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
  },
  { id: "codex", agent_kind: "codex", description: "codex on its own default model" },
  {
    id: "claude_code",
    agent_kind: "claude_code",
    description: "claude_code on its own default model",
  },
  {
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
  },
  { id: "opencode", agent_kind: "opencode", description: "opencode on its own default model" },
  { id: "opencode:zai-coding-plan/glm-4.6", agent_kind: "opencode", description: null },
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

it("offers the whole catalog, grouped by agent CLI", async () => {
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

it("leads each group with the agent CLI on its own", async () => {
  const user = userEvent.setup()
  const box = renderPicker()

  await user.click(box)
  const ids = within(await listbox())
    .getAllByRole("option")
    .map((option) => option.textContent ?? "")

  expect(ids[0]).toContain("claude_code")
  expect(ids[1]).toContain("claude_code:claude-opus-5")
  expect(ids[2]).toContain("codex")
  expect(ids[3]).toContain("codex:gpt-5.3-codex")
})

it("puts the picked id — agent CLI and all — in the field", async () => {
  const user = userEvent.setup()
  const box = renderPicker()

  await user.click(box)
  await user.click(within(await listbox()).getByText("codex:gpt-5.3-codex"))

  expect(box.value).toBe("codex:gpt-5.3-codex")
  await waitFor(() => {
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  })
})

it("keeps free text, which is handed to the CLI as typed", async () => {
  const user = userEvent.setup()
  const box = renderPicker()

  await user.type(box, "opencode:ollama/llama3:8b")

  expect(box.value).toBe("opencode:ollama/llama3:8b")
})

it("stays a plain text field when the catalog never arrived", async () => {
  const user = userEvent.setup()
  const box = renderPicker({ catalog: false })

  await user.click(box)
  await user.type(box, "claude_code:claude-opus-5")

  expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  expect(box.value).toBe("claude_code:claude-opus-5")
})
