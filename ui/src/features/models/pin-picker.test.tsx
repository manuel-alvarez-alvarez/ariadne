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

import type { EffortDto, ModelDto } from "@/api"
import { aModel, anEffort } from "@/test/fixtures"

import { PinPicker } from "./pin-picker"

/** One reasoning effort, named by its id, the way every fixture entry wants it. */
function effort(id: string, overrides: Partial<EffortDto> = {}): EffortDto {
  return anEffort({ id, ...overrides })
}

/**
 * A slice of the daemon's concrete-model catalog. The picker groups it by
 * agent, in the order the catalog first names each one.
 */
const CATALOG: ModelDto[] = [
  aModel({
    id: "codex-acp:gpt-5.5",
    agent_id: "codex-acp",
    description: "Frontier reasoning: agentic loops",
    tier: "frontier",
    cost: 5,
    speed: 2,
    best_for: ["cross-subsystem design"],
    avoid_for: ["small scoped edits"],
    efforts: [
      effort("low"),
      effort("medium", { description: "Balanced reasoning for everyday work", default: true }),
      effort("high"),
      effort("xhigh"),
      effort("max"),
      effort("ultra"),
    ],
  }),
  aModel({
    id: "claude-code-acp:claude-sonnet-5",
    agent_id: "claude-code-acp",
    description: "Everyday coding",
    tier: "balanced",
    cost: 3,
    speed: 3,
    best_for: ["everyday coding"],
    efforts: [
      effort("low"),
      effort("medium"),
      effort("high", { description: "Deeper reasoning, worth the wait", default: true }),
      effort("xhigh"),
      effort("max"),
    ],
  }),
  aModel({
    id: "claude-code-acp:claude-haiku-4-5",
    agent_id: "claude-code-acp",
    description: "Fast and cheap",
    tier: "fast",
    cost: 1,
    speed: 5,
  }),
  aModel({
    // Discovered, with the variants that model was configured with: an
    // effort belongs to its own model and to nothing else.
    id: "opencode-acp:zai-coding-plan/glm-4.6",
    agent_id: "opencode-acp",
    efforts: [effort("thinking"), effort("non-thinking")],
  }),
]

const LABEL = "Reviewer 2 runs on"

/** The picker holding its own pin, the way a form field holds it. */
function renderPicker({
  model = "",
  effort = "",
  catalog = true,
}: {
  model?: string
  effort?: string
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

/** The catalog row for one model id, found by its own id text. */
async function modelRow(id: string): Promise<HTMLElement> {
  const row = within(await listbox())
    .getByText(id)
    .closest('[role="option"]')
  if (!row) throw new Error(`no model row for "${id}"`)
  return row as HTMLElement
}

/** The effort ids offered for whatever is pinned, in order — the label's own line. */
function efforts(): string[] {
  return screen
    .getAllByRole("radio")
    .map((radio) => radio.closest("label")?.querySelector("span")?.textContent?.trim() ?? "")
}

/**
 * The radio for one effort, found by its own id line rather than by accessible
 * name — which a description joins once it has one.
 */
function effortRadio(id: string): HTMLElement {
  const radio = screen
    .getAllByRole("radio")
    .find(
      (candidate) => candidate.closest("label")?.querySelector("span")?.textContent?.trim() === id,
    )
  if (!radio) throw new Error(`no effort radio labelled "${id}"`)
  return radio
}

/** The muted description line under one effort's id, or undefined where it has none. */
function effortDescription(id: string): string | undefined {
  const spans = effortRadio(id).closest("label")?.querySelectorAll("span")
  return spans && spans.length > 1 ? (spans.item(1).textContent ?? undefined) : undefined
}

async function openPicker(user: ReturnType<typeof userEvent.setup>) {
  await user.click(trigger())
  await listbox()
}

/** Types into the popover's search box, which is also the free-text field. */
async function search(user: ReturnType<typeof userEvent.setup>, text: string) {
  await user.type(screen.getByRole("combobox", { name: LABEL }), text)
}

it("offers only concrete catalog models, grouped by agent", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  const options = within(await listbox()).getAllByRole("option")
  const ids = options.map((option) => option.textContent ?? "")

  expect(within(await listbox()).getByText("claude-code-acp")).toBeDefined()
  expect(within(await listbox()).getByText("codex-acp")).toBeDefined()
  expect(within(await listbox()).getByText("opencode-acp")).toBeDefined()
  const catalogIds = ids.filter((id) => !id.startsWith("Other"))
  expect(catalogIds.some((id) => id.includes("claude-code-acp:claude-sonnet-5"))).toBe(true)
  expect(catalogIds.every((id) => id.includes(":"))).toBe(true)
  expect(ids.at(-1)).toContain("Other")
})

/**
 * A model turned off on the models screen is not a choice: the daemon refuses
 * it as a pin, and a row that can be picked and then refused on submit is
 * worse than a row that is not there.
 *
 * What a slot already holds is the exception. It was pinned while the model
 * was on, and a picker that dropped it could not say what the field holds.
 */
it("leaves out a model that is turned off, unless it is the one pinned", async () => {
  const user = userEvent.setup()
  const off = CATALOG.map((model) =>
    model.id === "claude-code-acp:claude-sonnet-5" ? { ...model, enabled: false } : model,
  )
  function Host({ model }: { model: string }) {
    return <PinPicker label={LABEL} model={model} effort="" onChange={() => {}} models={off} />
  }

  const view = render(<Host model="" />)
  await openPicker(user)
  expect(
    within(await listbox())
      .getAllByRole("option")
      .every((option) => !(option.textContent ?? "").includes("claude-sonnet-5")),
  ).toBe(true)
  expect(within(await listbox()).getByText("claude-code-acp:claude-haiku-4-5")).toBeDefined()

  // Pinned to it already: the row is back, because the field has to be able
  // to say what it holds.
  view.rerender(<Host model="claude-code-acp:claude-sonnet-5" />)
  expect(
    within(await listbox())
      .getAllByRole("option")
      .some((option) => (option.textContent ?? "").includes("claude-sonnet-5")),
  ).toBe(true)
})

it("shows tier and cost/speed pills for a curated model", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  const row = await modelRow("claude-code-acp:claude-sonnet-5")

  expect(within(row).getByText("balanced")).toBeDefined()
  expect(within(row).getByText("cost 3/5")).toBeDefined()
  expect(within(row).getByText("speed 3/5")).toBeDefined()
})

it("shows no pills for a model nothing knows the tier, cost or speed of", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  const row = await modelRow("opencode-acp:zai-coding-plan/glm-4.6")

  expect(within(row).queryByText("unknown")).toBeNull()
  expect(within(row).queryByText(/cost \d\/5/)).toBeNull()
  expect(within(row).queryByText(/speed \d\/5/)).toBeNull()
})

it("puts what the catalog says a model is and is not for in the row's tooltip", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  const row = await modelRow("codex-acp:gpt-5.5")

  expect(row.querySelector("[title]")?.getAttribute("title")).toBe(
    "best for: cross-subsystem design\navoid for: small scoped edits",
  )
})

it("finds a model by what the catalog says it is best for", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  await search(user, "cross-subsystem design")

  expect(within(await listbox()).getByText("codex-acp:gpt-5.5")).toBeDefined()
})

it("pins the picked id, agent and all, and stays open for the effort", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await user.click(within(await listbox()).getByText("codex-acp:gpt-5.5"))

  expect(pin.model).toBe("codex-acp:gpt-5.5")
  expect(await listbox()).toBeDefined()
  expect(reads()).toBe("codex-acp gpt-5.5")
})

it("picks with the keyboard: search, arrow, enter", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await search(user, "claude-sonnet")
  await user.keyboard("{Enter}")

  expect(pin.model).toBe("claude-code-acp:claude-sonnet-5")
})

it("offers the efforts of the pinned model, the agent's own first, and stores the pick", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-code-acp:claude-sonnet-5" })

  await openPicker(user)
  expect(efforts()).toEqual(["auto (high)", "low", "medium", "high", "xhigh", "max"])
  // The catalog's own words for what the effort buys, muted under its id.
  expect(effortDescription("high")).toBe("Deeper reasoning, worth the wait")
  expect(effortDescription("medium")).toBeUndefined()

  await user.click(effortRadio("medium"))
  expect(pin.effort).toBe("medium")
  expect(reads()).toBe("claude-code-acp claude-sonnet-5 · medium")

  await user.click(effortRadio("auto (high)"))
  expect(pin.effort).toBe("")
})

it("shows no strip, and says why, for a model that takes no effort at all", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "claude-code-acp:claude-haiku-4-5" })

  await openPicker(user)

  expect(screen.queryAllByRole("radio")).toHaveLength(0)
  expect(await screen.findByText(/takes no effort at all/)).toBeDefined()
})

it("takes free text for a model nothing has discovered", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "opencode-acp:ollama/llama3:8b" })

  await openPicker(user)
  await user.type(screen.getByRole("textbox", { name: "Effort" }), "reasoning-high")

  expect(pin.effort).toBe("reasoning-high")
})

// A model the catalog does not list takes whichever effort its agent was
// configured with, and only that agent knows the list: the daemon takes any
// effort that is not blank, so the strip holds it to nothing either.
it("takes free text for a model of a known agent the catalog does not list", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "codex-acp:gpt-5.9-unreleased" })

  await openPicker(user)

  expect(screen.queryAllByRole("radio")).toHaveLength(0)
  expect(screen.getByRole("textbox", { name: "Effort" })).toBeDefined()
})

it("drops an effort the model moved to does not take", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-code-acp:claude-sonnet-5", effort: "medium" })

  // The glm model runs at the variants it alone was configured with, and
  // `medium` is not one of them.
  await openPicker(user)
  await user.click(within(await listbox()).getByText("opencode-acp:zai-coding-plan/glm-4.6"))

  expect(pin).toEqual({ model: "opencode-acp:zai-coding-plan/glm-4.6", effort: "" })
})

it("drops an effort where the model moved to takes none at all", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-code-acp:claude-sonnet-5", effort: "medium" })

  await openPicker(user)
  await user.click(within(await listbox()).getByText("claude-code-acp:claude-haiku-4-5"))

  expect(pin).toEqual({ model: "claude-code-acp:claude-haiku-4-5", effort: "" })
})

it("keeps an effort the model moved to takes as well", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-code-acp:claude-sonnet-5", effort: "max" })

  await openPicker(user)
  await user.click(within(await listbox()).getByText("codex-acp:gpt-5.5"))

  expect(pin).toEqual({ model: "codex-acp:gpt-5.5", effort: "max" })
})

it("requires a model before it offers effort choices", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)

  expect(screen.queryAllByRole("radio")).toHaveLength(0)
  expect(screen.getByText(/choose one first/)).toBeDefined()
})

it("asks the user to choose a model before one is picked", () => {
  renderPicker()

  expect(reads()).toBe("Choose a model")
})

it("takes a model the catalog does not carry, as typed", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await search(user, "claude-code-acp:some-future-model")
  await user.click(screen.getByText(/^Other — run/))

  expect(pin.model).toBe("claude-code-acp:some-future-model")
})

it("says why a typed id is no model reference, and pins it anyway for the field to refuse", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await search(user, "foo")

  expect(await screen.findByText(/"foo" is one half/)).toBeDefined()
  await user.click(screen.getByText(/^Other — run/))
  expect(pin.model).toBe("foo")
})

it("still takes free text when the catalog never arrived", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ catalog: false })

  await openPicker(user)
  await search(user, "claude-code-acp:claude-opus-5")
  await user.click(screen.getByText(/^Other — run/))

  expect(pin.model).toBe("claude-code-acp:claude-opus-5")
})

it("closes on Escape, leaving the pin as it was", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-code-acp:claude-sonnet-5", effort: "medium" })

  await openPicker(user)
  await user.keyboard("{Escape}")

  await waitFor(() => {
    expect(screen.queryByRole("listbox", { name: "Models" })).toBeNull()
  })
  expect(pin).toEqual({ model: "claude-code-acp:claude-sonnet-5", effort: "medium" })
})
