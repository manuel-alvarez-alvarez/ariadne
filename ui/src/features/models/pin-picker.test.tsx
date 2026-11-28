// @vitest-environment jsdom

/**
 * The pin picker's columns: how a pick moves through agent, model and effort, the
 * "Show disabled" switch, and the search.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"
import {
  CATALOG,
  heads,
  openPicker,
  option,
  popup,
  reads,
  renderPicker,
  resetPickerSettings,
  search,
  selected,
  showDisabled,
  stubIcons,
  trigger,
} from "@/test/pin-picker-kit"
import { PinPicker } from "./pin-picker"

beforeEach(resetPickerSettings)

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
  rerender(
    <PinPicker
      host="local"
      label="Other"
      model=""
      effort=""
      onChange={() => {}}
      models={CATALOG}
    />,
  )
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
