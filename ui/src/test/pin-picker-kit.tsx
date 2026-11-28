/**
 * What the pin picker's test files share: a catalog slice, the icon stub, a
 * picker that holds its own pin, and readers of its trigger and columns.
 */

import { screen, within } from "@testing-library/react"
import type userEvent from "@testing-library/user-event"
import { useState } from "react"

import type { EffortDto, ModelDto } from "@/api"
import { PinPicker } from "@/features/models/pin-picker"
import { useSettingsStore } from "@/stores/settings"
import { aModel, anAcpAgent, anEffort } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

/** One reasoning effort, named by its id, the way every fixture entry wants it. */
export function effort(id: string, overrides: Partial<EffortDto> = {}): EffortDto {
  return anEffort({ id, ...overrides })
}

/**
 * A slice of the daemon's concrete-model catalog. The picker lists its agents
 * in the order the catalog first names each one.
 */
export const CATALOG: ModelDto[] = [
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

export const LABEL = "Reviewer 2 runs on"
/** Answers the icon list, and nothing else: the catalog is handed in. */
export function stubIcons(icons: Record<string, string> = {}) {
  daemonFetch.mockImplementation(async (input: Request | string | URL) => {
    const url = new URL(input instanceof Request ? input.url : String(input))
    if (url.pathname === "/v1/acp-agents") {
      return jsonResponse(Object.entries(icons).map(([id, icon]) => anAcpAgent({ id, icon })))
    }
    return jsonResponse([])
  })
}

/** The picker holding its own pin, the way a form field holds it. */
export function renderPicker({
  model = "",
  effort = "",
  catalog = true,
}: {
  model?: string
  effort?: string
  catalog?: boolean
} = {}) {
  const pin = { model, effort }
  // biome-ignore lint/correctness/noComponentHookFactories: the component closes over this render's pin

  function Host() {
    const [value, setValue] = useState({ model, effort })
    pin.model = value.model
    pin.effort = value.effort
    return (
      <PinPicker
        host="local"
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
export function trigger(): HTMLElement {
  return screen.getByRole("button", { name: LABEL })
}

/** The trigger's text, without the decorative icon. */
export function reads(): string {
  const copy = trigger().cloneNode(true) as HTMLElement
  for (const icon of copy.querySelectorAll("[aria-hidden]")) icon.remove()
  return (copy.textContent ?? "").replace(/\s+/g, " ").trim()
}

export function popup(): HTMLElement | null {
  return screen.queryByRole("dialog", { name: LABEL })
}

export function column(name: "Agents" | "Models" | "Efforts"): HTMLElement {
  return screen.getByRole("listbox", { name })
}

/** One option's own first line: an agent's id, a model's id or an effort's label. */
export function head(option: HTMLElement): string {
  return option.querySelector("[data-slot=option-label]")?.textContent?.trim() ?? ""
}

export function heads(name: "Agents" | "Models" | "Efforts"): string[] {
  return within(column(name)).queryAllByRole("option").map(head)
}

export function option(name: "Agents" | "Models" | "Efforts", label: string): HTMLElement {
  const found = within(column(name))
    .queryAllByRole("option")
    .find((candidate) => head(candidate) === label)
  if (!found) throw new Error(`no ${name} option "${label}"`)
  return found
}

export function selected(name: "Agents" | "Models" | "Efforts"): string[] {
  return within(column(name))
    .queryAllByRole("option")
    .filter((candidate) => candidate.getAttribute("aria-selected") === "true")
    .map(head)
}

export async function openPicker(user: ReturnType<typeof userEvent.setup>) {
  await user.click(trigger())
  await screen.findByRole("listbox", { name: "Agents" })
}

export async function search(user: ReturnType<typeof userEvent.setup>, text: string) {
  await user.type(screen.getByRole("searchbox", { name: "Search models" }), text)
}

export function showDisabled(): HTMLElement {
  return screen.getByRole("switch", { name: "Show disabled" })
}

/** Forget the "Show disabled" switch a test left on. */
export function resetPickerSettings() {
  localStorage.clear()
  useSettingsStore.setState({ showDisabledModels: false })
}
