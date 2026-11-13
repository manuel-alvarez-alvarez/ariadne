// @vitest-environment jsdom

/**
 * The one control every form pins a slot with: the model, and the effort that
 * model is run at, behind a single trigger.
 *
 * The popover is three columns — agent, model, effort — under a search box
 * and a "Show disabled" switch. What is asserted here is how a choice moves
 * through them, and the daemon's own rules said before the round trip: the
 * effort is the *model's* closed list, and an effort never outlives a model
 * that cannot be run at it.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { beforeEach, expect, it } from "vitest"

import type { EffortDto, ModelDto } from "@/api"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import { useSettingsStore } from "@/stores/settings"
import { aModel, anAcpAgent, anEffort } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { PinPicker } from "./pin-picker"

/** One reasoning effort, named by its id, the way every fixture entry wants it. */
function effort(id: string, overrides: Partial<EffortDto> = {}): EffortDto {
  return anEffort({ id, ...overrides })
}

/**
 * A slice of the daemon's concrete-model catalog. The picker lists its agents
 * in the order the catalog first names each one.
 */
const CATALOG: ModelDto[] = [
  aModel({
    id: "codex-acp:gpt-5.5",
    agent_id: "codex-acp",
    description: "Frontier reasoning: agentic loops",
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
    id: "claude-agent-acp:claude-sonnet-5",
    agent_id: "claude-agent-acp",
    description: "Everyday coding",
    efforts: [
      effort("low"),
      effort("medium"),
      effort("high", { description: "Deeper reasoning, worth the wait", default: true }),
      effort("xhigh"),
      effort("max"),
    ],
  }),
  aModel({
    id: "claude-agent-acp:claude-haiku-4-5",
    agent_id: "claude-agent-acp",
    description: "Fast and cheap",
  }),
  aModel({
    id: "claude-agent-acp:claude-opus-3",
    agent_id: "claude-agent-acp",
    description: "Retired from automatic staffing",
    enabled: false,
  }),
  aModel({
    // Discovered, with the variants that model was configured with: an
    // effort belongs to its own model and to nothing else.
    id: "opencode-acp:zai-coding-plan/glm-4.6",
    agent_id: "opencode-acp",
    efforts: [effort("thinking"), effort("non-thinking")],
  }),
  aModel({
    // An agent whose every model is turned off.
    id: "gemini:gemini-3-pro",
    agent_id: "gemini",
    description: "Long context",
    enabled: false,
  }),
]

const LABEL = "Reviewer 2 runs on"

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ showDisabledModels: false })
})

/** Answers the icon list, and nothing else: the catalog is handed in. */
function stubIcons(icons: Record<string, string> = {}) {
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const url = new URL(input instanceof Request ? input.url : String(input))
    if (url.pathname === "/v1/acp-agents") {
      return jsonResponse(Object.entries(icons).map(([id, icon]) => anAcpAgent({ id, icon })))
    }
    return jsonResponse([])
  })
}

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
  const view = renderScreen(<Host />)
  return Object.assign(pin, { rerender: view.rerender })
}

/** The trigger, which is also what the whole choice reads back from. */
function trigger(): HTMLElement {
  return screen.getByRole("button", { name: LABEL })
}

/** The trigger's text, without the decorative icon. */
function reads(): string {
  const copy = trigger().cloneNode(true) as HTMLElement
  for (const icon of copy.querySelectorAll("[aria-hidden]")) icon.remove()
  return (copy.textContent ?? "").replace(/\s+/g, " ").trim()
}

function popup(): HTMLElement | null {
  return screen.queryByRole("dialog", { name: LABEL })
}

function column(name: "Agents" | "Models" | "Efforts"): HTMLElement {
  return screen.getByRole("listbox", { name })
}

/** One option's own first line: an agent's id, a model's id or an effort's label. */
function head(option: HTMLElement): string {
  return option.querySelector("[data-slot=option-label]")?.textContent?.trim() ?? ""
}

function heads(name: "Agents" | "Models" | "Efforts"): string[] {
  return within(column(name)).queryAllByRole("option").map(head)
}

function option(name: "Agents" | "Models" | "Efforts", label: string): HTMLElement {
  const found = within(column(name))
    .queryAllByRole("option")
    .find((candidate) => head(candidate) === label)
  if (!found) throw new Error(`no ${name} option "${label}"`)
  return found
}

function selected(name: "Agents" | "Models" | "Efforts"): string[] {
  return within(column(name))
    .queryAllByRole("option")
    .filter((candidate) => candidate.getAttribute("aria-selected") === "true")
    .map(head)
}

async function openPicker(user: ReturnType<typeof userEvent.setup>) {
  await user.click(trigger())
  await screen.findByRole("listbox", { name: "Agents" })
}

async function search(user: ReturnType<typeof userEvent.setup>, text: string) {
  await user.type(screen.getByRole("searchbox", { name: "Search models" }), text)
}

function showDisabled(): HTMLElement {
  return screen.getByRole("switch", { name: "Show disabled" })
}

it("asks the user to choose a model before one is picked", () => {
  renderPicker()

  expect(reads()).toBe("Choose a model")
})

it("shows the agent icon, the model and the effort on the trigger", () => {
  renderPicker({ model: "codex-acp:gpt-5.5", effort: "high" })

  expect(trigger().querySelector("img")?.getAttribute("src")).toBe("/agent-icons/codex.svg")
  expect(reads()).toBe("gpt-5.5 · high")
})

it("puts a search box and a Show disabled switch above the agent, model and effort columns", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  const dialog = popup()
  if (!dialog) throw new Error("the popover is not open")
  const order = [
    ...dialog.querySelectorAll('input[type="search"], [role="switch"], [role="listbox"]'),
  ].map(
    (node) =>
      node.getAttribute("aria-label") ??
      node.getAttribute("role") ??
      (node as HTMLInputElement).type,
  )
  expect(order).toEqual(["Search models", "switch", "Agents", "Models", "Efforts"])
  expect(showDisabled().getAttribute("aria-checked")).toBe("false")
})

it("lists each agent with a shown model, with its icon, in catalog order", async () => {
  stubIcons({ "opencode-acp": "https://cdn.example/opencode-acp.svg" })
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)

  // Gemini has only a turned-off model, so the switch keeps it out.
  expect(heads("Agents")).toEqual(["codex-acp", "claude-agent-acp", "opencode-acp"])
  expect(option("Agents", "codex-acp").querySelector("img")?.getAttribute("src")).toBe(
    "/agent-icons/codex.svg",
  )
  await waitFor(() => {
    expect(option("Agents", "opencode-acp").querySelector("img")?.getAttribute("src")).toBe(
      "https://cdn.example/opencode-acp.svg",
    )
  })
})

it("fills the model column with the picked agent's models, each with its id and description", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  await user.click(option("Agents", "claude-agent-acp"))

  expect(heads("Models")).toEqual([
    "claude-agent-acp:claude-sonnet-5",
    "claude-agent-acp:claude-haiku-4-5",
  ])
  expect(option("Models", "claude-agent-acp:claude-sonnet-5").textContent).toBe(
    "claude-agent-acp:claude-sonnet-5Everyday coding",
  )
})

it("fills the effort column from the picked model, and an effort pick completes the choice", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await user.click(option("Agents", "claude-agent-acp"))
  await user.click(option("Models", "claude-agent-acp:claude-sonnet-5"))

  expect(heads("Efforts")).toEqual(["auto (high)", "low", "medium", "high", "xhigh", "max"])
  // The catalog's own words for what the effort buys, under its label.
  expect(option("Efforts", "high").textContent).toBe("highDeeper reasoning, worth the wait")
  expect(option("Efforts", "medium").textContent).toBe("medium")

  await user.click(option("Efforts", "medium"))

  expect(pin).toMatchObject({ model: "claude-agent-acp:claude-sonnet-5", effort: "medium" })
  await waitFor(() => expect(popup()).toBeNull())
  expect(reads()).toBe("claude-sonnet-5 · medium")
})

it("picks the agent's own effort as auto", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-agent-acp:claude-sonnet-5", effort: "medium" })

  await openPicker(user)
  await user.click(option("Efforts", "auto (high)"))

  expect(pin.effort).toBe("")
})

it("opens on the current pin, with its agent, model and effort selected", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "claude-agent-acp:claude-sonnet-5", effort: "xhigh" })

  await openPicker(user)

  expect(selected("Agents")).toEqual(["claude-agent-acp"])
  expect(selected("Models")).toEqual(["claude-agent-acp:claude-sonnet-5"])
  expect(selected("Efforts")).toEqual(["xhigh"])
})

it("shows turned-off models only with the switch on, muted and marked off", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  await user.click(option("Agents", "claude-agent-acp"))
  expect(heads("Models")).not.toContain("claude-agent-acp:claude-opus-3")

  await user.click(showDisabled())

  expect(heads("Agents")).toEqual(["codex-acp", "claude-agent-acp", "opencode-acp", "gemini"])
  // In catalog order, among the turned-on ones rather than after them.
  expect(heads("Models")).toEqual([
    "claude-agent-acp:claude-sonnet-5",
    "claude-agent-acp:claude-haiku-4-5",
    "claude-agent-acp:claude-opus-3",
  ])
  const off = option("Models", "claude-agent-acp:claude-opus-3")
  expect(within(off).getByText("off")).toBeDefined()
  expect(off.className).toContain("text-muted-foreground")
  const on = option("Models", "claude-agent-acp:claude-sonnet-5")
  expect(within(on).queryByText("off")).toBeNull()
  expect(screen.queryByText(/not enabled|manual only/i)).toBeNull()
})

it("opens a turned-off pin with the switch turned on, and hides it once the switch is off", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-agent-acp:claude-opus-3" })

  await openPicker(user)

  expect(showDisabled().getAttribute("aria-checked")).toBe("true")
  expect(selected("Agents")).toEqual(["claude-agent-acp"])
  expect(selected("Models")).toEqual(["claude-agent-acp:claude-opus-3"])

  await user.click(showDisabled())

  expect(heads("Models")).toEqual([
    "claude-agent-acp:claude-sonnet-5",
    "claude-agent-acp:claude-haiku-4-5",
  ])
  expect(selected("Models")).toEqual([])
  expect(heads("Efforts")).toEqual([])
  // Hiding the model is not a repin.
  expect(pin.model).toBe("claude-agent-acp:claude-opus-3")
})

it("keeps the switch across opens, pickers and restarts", async () => {
  const user = userEvent.setup()
  const { rerender } = renderPicker()

  await openPicker(user)
  await user.click(showDisabled())
  await user.keyboard("{Escape}")
  await waitFor(() => expect(popup()).toBeNull())

  await openPicker(user)
  expect(showDisabled().getAttribute("aria-checked")).toBe("true")
  await user.keyboard("{Escape}")
  await waitFor(() => expect(popup()).toBeNull())

  // Another picker, on another form.
  rerender(<PinPicker label="Other" model="" effort="" onChange={() => {}} models={CATALOG} />)
  await user.click(screen.getByRole("button", { name: "Other" }))
  expect(
    (await screen.findByRole("switch", { name: "Show disabled" })).getAttribute("aria-checked"),
  ).toBe("true")

  // And what a restart reads back.
  const stored = JSON.parse(localStorage.getItem("ariadne.settings") ?? "{}")
  expect(stored.state.showDisabledModels).toBe(true)
})

it("replaces the agent and model columns with one list of every matching model, on and off", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  await search(user, "claude")

  expect(screen.queryByRole("listbox", { name: "Agents" })).toBeNull()
  // The switch is off, and the turned-off model is listed anyway.
  expect(heads("Models")).toEqual([
    "claude-agent-acp:claude-sonnet-5",
    "claude-agent-acp:claude-haiku-4-5",
    "claude-agent-acp:claude-opus-3",
  ])
  const row = option("Models", "claude-agent-acp:claude-sonnet-5")
  expect(row.querySelector('[aria-hidden="true"]')?.textContent).toBe("c")
  expect(within(row).getByText("Everyday coding")).toBeDefined()
  const off = option("Models", "claude-agent-acp:claude-opus-3")
  expect(off.className).toContain("text-muted-foreground")
  expect(within(off).getByText("off")).toBeDefined()
})

it("finds a model by what its agent said about it", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)
  await search(user, "agentic loops")

  expect(heads("Models")).toEqual(["codex-acp:gpt-5.5"])
})

it("clears the search on a pick and shows the columns on that agent and model", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await search(user, "long context")
  await user.click(option("Models", "gemini:gemini-3-pro"))

  expect(pin.model).toBe("gemini:gemini-3-pro")
  expect((screen.getByRole("searchbox", { name: "Search models" }) as HTMLInputElement).value).toBe(
    "",
  )
  expect(selected("Agents")).toEqual(["gemini"])
  expect(selected("Models")).toEqual(["gemini:gemini-3-pro"])
  // A turned-off pick turns the switch on, so the columns can show it.
  expect(showDisabled().getAttribute("aria-checked")).toBe("true")

  await user.click(showDisabled())

  expect(heads("Agents")).toEqual(["codex-acp", "claude-agent-acp", "opencode-acp"])
  expect(screen.queryByText("gemini:gemini-3-pro")).toBeNull()
})

it("offers no free-text row, and keeps a saved pin the catalog does not carry on the trigger", async () => {
  const user = userEvent.setup()
  renderPicker({ model: "claude-agent-acp:some-future-model", effort: "high" })

  expect(reads()).toBe("some-future-model · high")
  await openPicker(user)
  await search(user, "claude-agent-acp:some-future-model")

  expect(screen.queryByText(/Other/)).toBeNull()
  expect(screen.getByText("Nothing in the catalog matches.")).toBeDefined()
})

it("drops an effort the model moved to does not take", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-agent-acp:claude-sonnet-5", effort: "medium" })

  // The glm model runs at the variants it alone was configured with, and
  // `medium` is not one of them.
  await openPicker(user)
  await user.click(option("Agents", "opencode-acp"))
  await user.click(option("Models", "opencode-acp:zai-coding-plan/glm-4.6"))

  expect(pin).toMatchObject({ model: "opencode-acp:zai-coding-plan/glm-4.6", effort: "" })
})

it("keeps an effort the model moved to takes as well", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-agent-acp:claude-sonnet-5", effort: "max" })

  await openPicker(user)
  await user.click(option("Agents", "codex-acp"))
  await user.click(option("Models", "codex-acp:gpt-5.5"))

  expect(pin).toMatchObject({ model: "codex-acp:gpt-5.5", effort: "max" })
})

it("drops the effort of a model that takes none, and says so", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "claude-agent-acp:claude-sonnet-5", effort: "medium" })

  await openPicker(user)
  await user.click(option("Models", "claude-agent-acp:claude-haiku-4-5"))

  expect(pin).toMatchObject({ model: "claude-agent-acp:claude-haiku-4-5", effort: "" })
  expect(heads("Efforts")).toEqual([])
  expect(screen.getByText(/takes no effort at all/)).toBeDefined()
})

it("requires a model before it offers effort choices", async () => {
  const user = userEvent.setup()
  renderPicker()

  await openPicker(user)

  expect(heads("Efforts")).toEqual([])
  expect(screen.getByText(/choose one first/)).toBeDefined()
})

// A model the catalog does not list takes whichever effort its agent was
// configured with, and only that agent knows the list: the daemon takes any
// effort that is not blank, so the column holds it to nothing either.
it("takes free text for a model of a known agent the catalog does not list", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "codex-acp:gpt-5.9-unreleased" })

  await openPicker(user)
  await user.type(screen.getByRole("textbox", { name: "Effort" }), "reasoning-high")

  expect(pin.effort).toBe("reasoning-high")
})

it("takes free text for a model nothing has discovered", async () => {
  const user = userEvent.setup()
  const pin = renderPicker({ model: "opencode-acp:ollama/llama3:8b", catalog: false })

  await openPicker(user)
  await user.type(screen.getByRole("textbox", { name: "Effort" }), "reasoning-high")

  expect(pin.effort).toBe("reasoning-high")
})

it("picks with the keyboard alone: search, arrows, enter", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await search(user, "sonnet")
  await user.keyboard("{Enter}")
  expect(pin.model).toBe("claude-agent-acp:claude-sonnet-5")

  // The pick hands the keyboard to the efforts, on the agent's own.
  expect(document.activeElement).toBe(option("Efforts", "auto (high)"))
  await user.keyboard("{ArrowDown}{ArrowDown}{Enter}")

  expect(pin.effort).toBe("medium")
  await waitFor(() => expect(popup()).toBeNull())
})

it("moves across the columns with the arrow keys", async () => {
  const user = userEvent.setup()
  const pin = renderPicker()

  await openPicker(user)
  await user.click(option("Agents", "codex-acp"))
  expect(document.activeElement).toBe(option("Models", "codex-acp:gpt-5.5"))

  await user.keyboard("{ArrowLeft}")
  expect(document.activeElement).toBe(option("Agents", "codex-acp"))
  await user.keyboard("{ArrowDown}{ArrowRight}")
  expect(selected("Agents")).toEqual(["claude-agent-acp"])
  expect(document.activeElement).toBe(option("Models", "claude-agent-acp:claude-sonnet-5"))

  await user.keyboard("{ArrowDown}{Enter}")
  expect(pin.model).toBe("claude-agent-acp:claude-haiku-4-5")
})

it("closes on Escape, and leaves the form dialog open", async () => {
  const user = userEvent.setup()
  let pin = { model: "claude-agent-acp:claude-sonnet-5", effort: "medium" }
  renderScreen(
    <Dialog open onOpenChange={() => {}}>
      <DialogContent>
        <DialogTitle>Form</DialogTitle>
        <PinPicker
          label={LABEL}
          model={pin.model}
          effort={pin.effort}
          onChange={(next) => {
            pin = next
          }}
          models={CATALOG}
        />
      </DialogContent>
    </Dialog>,
  )
  // What a form dialog's own Escape would hear, bubbling up to the document.
  let heard = false
  const listen = (event: KeyboardEvent) => {
    if (event.key === "Escape") heard = true
  }
  document.addEventListener("keydown", listen)

  await openPicker(user)
  await user.keyboard("{Escape}")
  document.removeEventListener("keydown", listen)

  await waitFor(() => expect(popup()).toBeNull())
  expect(heard).toBe(false)
  expect(screen.getByRole("dialog", { name: "Form" })).toBeDefined()
  expect(pin).toEqual({ model: "claude-agent-acp:claude-sonnet-5", effort: "medium" })
})
