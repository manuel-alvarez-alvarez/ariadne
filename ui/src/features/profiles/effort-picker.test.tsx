// @vitest-environment jsdom

/**
 * The effort field beside a model box: a closed list, scoped by the model.
 *
 * What it offers is the catalog's answer for whatever the model box holds —
 * the model's own efforts where it lists them, everything the agent CLI
 * accepts where it does not, and free text where nothing can know (an opencode
 * model discovery has not seen). The first entry is always `auto`, which is no
 * effort pinned, named with what the CLI runs the model at where the catalog
 * says so.
 */

import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it } from "vitest"

import type { ModelDto } from "@/api"

import { EffortPicker } from "./effort-picker"

/** A slice of the daemon's catalog, with the efforts each entry is run at. */
const CATALOG: ModelDto[] = [
  {
    id: "claude_code",
    agent_kind: "claude_code",
    description: "claude_code on its own default model",
    efforts: [],
    default_effort: null,
  },
  {
    id: "claude_code:claude-opus-5",
    agent_kind: "claude_code",
    description: "Opus tier: deep analysis",
    efforts: ["low", "medium", "high", "xhigh", "max"],
    default_effort: "high",
  },
  {
    id: "claude_code:claude-haiku-4-5",
    agent_kind: "claude_code",
    description: "Fast and cheap",
    efforts: [],
    default_effort: null,
  },
  {
    id: "codex:gpt-5.6-luna",
    agent_kind: "codex",
    description: "Fast and cheapest",
    efforts: ["low", "medium", "high", "xhigh", "max"],
    default_effort: "medium",
  },
  {
    id: "codex:gpt-5.6-sol",
    agent_kind: "codex",
    description: "Flagship reasoning",
    efforts: ["low", "medium", "high", "xhigh", "max", "ultra"],
    default_effort: "medium",
  },
  {
    id: "opencode",
    agent_kind: "opencode",
    description: "opencode on its own default model",
    efforts: [],
    default_effort: null,
  },
  {
    // Discovered, with the variants that model was configured with: an
    // opencode effort belongs to its own model and to nothing else.
    id: "opencode:zai-coding-plan/glm-4.6",
    agent_kind: "opencode",
    description: null,
    efforts: ["thinking", "non-thinking"],
    default_effort: null,
  },
]

/** The picker holding its own value, the way a form field holds it. */
function renderPicker({
  model,
  effort = "",
  catalog = true,
}: {
  model: string
  effort?: string
  catalog?: boolean
}) {
  const chosen = { effort }
  function Host() {
    const [value, setValue] = useState(effort)
    const [pinned, setPinned] = useState(model)
    chosen.effort = value
    return (
      <>
        <button type="button" onClick={() => setPinned("claude_code:claude-haiku-4-5")}>
          Move the model
        </button>
        <EffortPicker
          value={value}
          onChange={(next) => {
            chosen.effort = next
            setValue(next)
          }}
          model={pinned}
          models={catalog ? CATALOG : undefined}
        />
      </>
    )
  }
  render(<Host />)
  return chosen
}

/** The options the list offers, in order. */
async function options(user: ReturnType<typeof userEvent.setup>): Promise<string[]> {
  await user.click(screen.getByLabelText("Effort"))
  return (await screen.findAllByRole("option")).map((option) => option.textContent ?? "")
}

it("offers the efforts of the model beside it, auto first", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "claude_code:claude-opus-5" })

  expect(await options(user)).toEqual(["auto (high)", "low", "medium", "high", "xhigh", "max"])
})

it("names what the CLI runs the model at, so auto is a choice and not a blank", async () => {
  renderPicker({ model: "codex:gpt-5.6-luna" })

  expect(screen.getByLabelText("Effort").textContent).toContain("auto (medium)")
})

it("stores the picked effort, and the empty string for auto", async () => {
  const user = userEvent.setup()
  const chosen = renderPicker({ model: "claude_code:claude-opus-5" })

  await user.click(screen.getByLabelText("Effort"))
  await user.click(await screen.findByRole("option", { name: "xhigh" }))
  expect(chosen.effort).toBe("xhigh")

  await user.click(screen.getByLabelText("Effort"))
  await user.click(await screen.findByRole("option", { name: "auto (high)" }))
  expect(chosen.effort).toBe("")
})

it("offers everything the agent CLI takes for a model the catalog does not list", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "codex:gpt-5.9-unreleased" })

  // The union of the codex entries, in the order the catalog lists them, and
  // no default to name: which model that is, nothing here knows.
  expect(await options(user)).toEqual(["auto", "low", "medium", "high", "xhigh", "max", "ultra"])
})

it("does the same for an agent CLI pinned on its own default model", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "claude_code" })

  expect(await options(user)).toEqual(["auto", "low", "medium", "high", "xhigh", "max"])
})

it("is disabled, and says why, for a model that takes no effort at all", () => {
  renderPicker({ model: "claude_code:claude-haiku-4-5" })

  const field = screen.getByLabelText("Effort")
  expect(field).toHaveProperty("disabled", true)
  expect(field.getAttribute("title")).toContain("takes no effort")
})

it("is disabled while no model is chosen, since an effort is run at one", () => {
  renderPicker({ model: "" })

  const field = screen.getByLabelText("Effort")
  expect(field).toHaveProperty("disabled", true)
  expect(field.getAttribute("title")).toContain("choose one first")
})

it("offers a discovered opencode model the variants it was configured with", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "opencode:zai-coding-plan/glm-4.6" })

  expect(await options(user)).toEqual(["auto", "thinking", "non-thinking"])
})

/**
 * The variants of one opencode model say nothing about another: they are that
 * model's own, discovered from it, where claude_code's and codex's belong to
 * the CLI. So an id discovery has not seen is free text even with other
 * opencode models in the catalog — which is how the daemon reads one too.
 */
it("takes free text for an opencode model nothing has discovered", async () => {
  const user = userEvent.setup()
  const chosen = renderPicker({ model: "opencode:ollama/llama3:8b" })

  await user.type(screen.getByLabelText("Effort"), "reasoning-high")

  expect(chosen.effort).toBe("reasoning-high")
})

it("is disabled with no model even while the catalog is still on its way", () => {
  // That there is no model to run an effort at is a fact about the box beside
  // it, not about the catalog: an enabled box here would take an effort the
  // daemon refuses outright.
  renderPicker({ model: "", catalog: false })

  const field = screen.getByLabelText("Effort")
  expect(field).toHaveProperty("disabled", true)
  expect(field.getAttribute("title")).toContain("choose one first")
})

it("stays free text while the catalog has not arrived", async () => {
  const user = userEvent.setup()
  const chosen = renderPicker({ model: "claude_code:claude-opus-5", catalog: false })

  await user.type(screen.getByLabelText("Effort"), "max")

  expect(chosen.effort).toBe("max")
})

it("drops back to auto when the model moves to one that does not take the effort", async () => {
  const user = userEvent.setup()
  const chosen = renderPicker({ model: "codex:gpt-5.6-sol", effort: "ultra" })

  expect(screen.getByLabelText("Effort").textContent).toContain("ultra")

  await user.click(screen.getByRole("button", { name: "Move the model" }))

  expect(chosen.effort).toBe("")
})
