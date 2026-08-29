// @vitest-environment jsdom

/**
 * The one control every form pins a slot with: the model, and the effort that
 * model is run at, behind a single trigger.
 *
 * What is asserted here is the pair's behaviour, which is the daemon's own set
 * of rules said before the round trip: the model is free text the catalog only
 * suggests, the effort is the *model's* closed list, and an effort never
 * outlives a model that cannot be run at it.
 */

import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it } from "vitest"

import type { ModelDto } from "@/api"

import { PinPicker } from "./pin-picker"

/**
 * A slice of the daemon's catalog: each agent CLI on its own and models of it,
 * deliberately not in agent order, since the picker groups it itself.
 */
const CATALOG: ModelDto[] = [
  {
    id: "codex:gpt-5.5",
    agent_kind: "codex",
    description: "Frontier reasoning: agentic loops",
    efforts: ["low", "medium", "high", "xhigh", "max", "ultra"],
    default_effort: "medium",
  },
  {
    id: "codex",
    agent_kind: "codex",
    description: "codex on its own default model",
    efforts: [],
    default_effort: null,
  },
  {
    id: "claude_code",
    agent_kind: "claude_code",
    description: "claude_code on its own default model",
    efforts: [],
    default_effort: null,
  },
  {
    id: "claude_code:claude-sonnet-5",
    agent_kind: "claude_code",
    description: "Everyday coding",
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

const LABEL = "Reviewer 2 runs on"

/** The picker holding its own pin, the way a form field holds it. */
function renderPicker({
  model = "",
  effort = "",
  fallback = null,
  catalog = true,
}: {
  model?: string
  effort?: string
  fallback?: { model: string | null; effort: string | null } | null
  catalog?: boolean
} = {}) {
  const pin = { model, effort }
  function Host() {
    const [value, setValue] = useState({ model, effort })
    pin.model = value.model
    pin.effort = value.effort
    return (
      <PinPicker
        label={LABEL}
        model={value.model}
        effort={value.effort}
        onChange={setValue}
        models={catalog ? CATALOG : undefined}
        fallback={fallback}
      />
    )
  }
  render(<Host />)
  return pin
}

/** The trigger, which is also what the whole choice reads back from. */
function trigger(): HTMLElement {
  return screen.getByRole("button", { name: LABEL })
}

/** The trigger's text, with the whitespace a screen collapses collapsed. */
function reads(): string {
  return (trigger().textContent ?? "").replace(/\s+/g, " ").trim()
}

/** The catalog list, which lives in a portal outside the trigger. */
async function listbox(): Promise<HTMLElement> {
  return await screen.findByRole("listbox", { name: "Models" })
}

/** The efforts offered for whatever is pinned, in order. */
function efforts(): string[] {
  return screen
    .getAllByRole("radio")
    .map((radio) => (radio.closest("label")?.textContent ?? "").trim())
}

async function openPicker(user: ReturnType<typeof userEvent.setup>) {
  await user.click(trigger())
  await listbox()
}

/** Types into the popover's search box, which is also the free-text field. */
async function search(user: ReturnType<typeof userEvent.setup>, text: string) {
  await user.type(screen.getByRole("combobox", { name: LABEL }), text)
}

it("offers the whole catalog, grouped by agent CLI, each group led by the CLI itself", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  const options = within(await listbox()).getAllByRole("option")
  const ids = options.map((option) => option.textContent ?? "")

  expect(within(await listbox()).getByText("Claude Code")).toBeDefined()
  expect(within(await listbox()).getByText("Codex")).toBeDefined()
  expect(within(await listbox()).getByText("OpenCode")).toBeDefined()
  // The unpinned row first, then each group with the bare CLI id ahead of its
  // models, then the row that takes whatever was typed.
  expect(ids[1]).toContain("claude_code")
  expect(ids[2]).toContain("claude_code:claude-sonnet-5")
  expect(ids.at(-1)).toContain("Other")
})

it("pins the picked id, agent CLI and all, and stays open for the effort", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await user.click(within(await listbox()).getByText("codex:gpt-5.5"))

  expect(pin.model).toBe("codex:gpt-5.5")
  expect(await listbox()).toBeDefined()
  expect(reads()).toBe("Codex gpt-5.5")
})

it("picks with the keyboard: search, arrow, enter", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await search(user, "sonnet")
  // The row that hands the pin back is always the first, so the first arrow
  // lands on the first match.
  await user.keyboard("{ArrowDown}{Enter}")

  expect(pin.model).toBe("claude_code:claude-sonnet-5")
})

it("offers the efforts of the pinned model, the CLI's own first, and stores the pick", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude_code:claude-sonnet-5" })

  await openPicker(user)
  expect(efforts()).toEqual(["auto (high)", "low", "medium", "high", "xhigh", "max"])

  await user.click(screen.getByRole("radio", { name: "medium" }))
  expect(pin.effort).toBe("medium")
  expect(reads()).toBe("Claude Code claude-sonnet-5 · medium")

  await user.click(screen.getByRole("radio", { name: "auto (high)" }))
  expect(pin.effort).toBe("")
})

it("shows no strip, and says why, for a model that takes no effort at all", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "claude_code:claude-haiku-4-5" })

  await openPicker(user)

  expect(screen.queryAllByRole("radio")).toHaveLength(0)
  expect(await screen.findByText(/takes no effort at all/)).toBeDefined()
})

it("takes free text for an opencode model nothing has discovered", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "opencode:ollama/llama3:8b" })

  await openPicker(user)
  await user.type(screen.getByRole("textbox", { name: "Effort" }), "reasoning-high")

  expect(pin.effort).toBe("reasoning-high")
})

it("offers everything the agent CLI takes for a model the catalog does not list", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "codex:gpt-5.9-unreleased" })

  await openPicker(user)

  // The union of the codex entries, in the order the catalog lists them, and
  // no default to name: which model that is, nothing here knows.
  expect(efforts()).toEqual(["auto", "low", "medium", "high", "xhigh", "max", "ultra"])
})

it("drops an effort the model moved to does not take", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude_code:claude-sonnet-5", effort: "medium" })

  // An opencode model runs at the variants it alone was configured with, and
  // `medium` is not one of them.
  await openPicker(user)
  await user.click(within(await listbox()).getByText("opencode:zai-coding-plan/glm-4.6"))

  expect(pin).toEqual({ model: "opencode:zai-coding-plan/glm-4.6", effort: "" })
})

it("drops an effort where the model moved to takes none at all", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude_code:claude-sonnet-5", effort: "medium" })

  await openPicker(user)
  await user.click(within(await listbox()).getByText("claude_code:claude-haiku-4-5"))

  expect(pin).toEqual({ model: "claude_code:claude-haiku-4-5", effort: "" })
})

it("keeps an effort the model moved to takes as well", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude_code:claude-sonnet-5", effort: "max" })

  await openPicker(user)
  await user.click(within(await listbox()).getByText("codex:gpt-5.5"))

  expect(pin).toEqual({ model: "codex:gpt-5.5", effort: "max" })
})

it("hands the pin back to the profile, effort and all", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({
    model: "claude_code:claude-sonnet-5",
    effort: "medium",
    fallback: { model: "codex:gpt-5.6-luna", effort: null },
  })

  await openPicker(user)
  await user.click(within(await listbox()).getByText("Profile's own"))

  expect(pin).toEqual({ model: "", effort: "" })
  expect(reads()).toBe("Profile's own — codex:gpt-5.6-luna")
})

/**
 * The two halves are pinned separately because the daemon takes them
 * separately: an effort with no model beside it runs the model the slot would
 * have run on anyway, at that effort (`http/pins.rs`, `chosen`). So the strip
 * is offered against the profile's own model, and what it stores is an effort
 * with an empty model — which the trigger has to say out loud.
 */
it("pins an effort of its own over the profile's model, and says so", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ fallback: { model: "codex:gpt-5.5", effort: "high" } })

  await openPicker(user)
  expect(efforts()).toEqual(["auto (medium)", "low", "medium", "high", "xhigh", "max", "ultra"])

  await user.click(screen.getByRole("radio", { name: "medium" }))

  expect(pin).toEqual({ model: "", effort: "medium" })
  // The profile's own `high` is not what it is run at any more, so the line
  // names the model it runs on and the effort actually chosen.
  expect(reads()).toBe("Profile's own — codex:gpt-5.5 · medium")
})

it("takes that effort back with the row that hands the pin over", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ effort: "medium", fallback: { model: "codex:gpt-5.5", effort: null } })

  await openPicker(user)
  await user.click(within(await listbox()).getByText("Profile's own"))

  expect(pin).toEqual({ model: "", effort: "" })
  expect(reads()).toBe("Profile's own — codex:gpt-5.5")
})

it("has no effort to offer where nothing says what an empty pin runs on", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)

  // Auto is resolved at spawn time, so there is no model here for an effort to
  // be run at — which the daemon refuses outright.
  expect(screen.queryAllByRole("radio")).toHaveLength(0)
  expect(screen.getByText(/An effort is run at a model/)).toBeDefined()
})

it("says auto where nothing can name what an empty pin resolves to", () => {
  renderPicker()

  expect(reads()).toBe("auto")
})

it("takes a model the catalog does not carry, as typed", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await search(user, "claude_code:some-future-model")
  await user.click(screen.getByText(/^Other — run/))

  expect(pin.model).toBe("claude_code:some-future-model")
})

it("says why a typed id is no model reference, and pins it anyway for the field to refuse", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await search(user, "foo:bar")

  expect(await screen.findByText(/"foo" is no agent CLI/)).toBeDefined()
  await user.click(screen.getByText(/^Other — run/))
  expect(pin.model).toBe("foo:bar")
})

it("still takes free text when the catalog never arrived", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ catalog: false })

  await openPicker(user)
  await search(user, "claude_code:claude-opus-5")
  await user.click(screen.getByText(/^Other — run/))

  expect(pin.model).toBe("claude_code:claude-opus-5")
})

it("closes on Escape, leaving the pin as it was", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude_code:claude-sonnet-5", effort: "medium" })

  await openPicker(user)
  await user.keyboard("{Escape}")

  await waitFor(() => {
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  })
  expect(pin).toEqual({ model: "claude_code:claude-sonnet-5", effort: "medium" })
})
