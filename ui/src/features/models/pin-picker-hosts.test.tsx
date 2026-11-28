// @vitest-environment jsdom

/**
 * The pin picker with a host list: the host column, its disabled rows, and the
 * models of the picked host.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { beforeEach, expect, it } from "vitest"

import { aModel } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import {
  CATALOG,
  head,
  heads,
  LABEL,
  openPicker,
  option,
  reads,
  renderPicker,
  resetPickerSettings,
  search,
  selected,
  stubIcons,
  trigger,
} from "@/test/pin-picker-kit"
import { PinPicker } from "./pin-picker"

beforeEach(resetPickerSettings)

/** The picker with two hosts listed, `box` unreachable unless it says else; each host has its own catalog. */
function renderHostPicker({ reason = null as string | null, start = "local" } = {}) {
  const picks: { host: string; model: string; effort: string }[] = []
  const boxCatalog = [
    aModel({ id: "opencode-acp:box-model", agent_id: "opencode-acp", description: "Only on box" }),
  ]
  function Wrapper() {
    const [value, setValue] = useState({ host: start, model: "", effort: "" })
    return (
      <PinPicker
        host={value.host}
        hosts={[{ host: "local" }, { host: "box", reason }]}
        label={LABEL}
        model={value.model}
        effort={value.effort}
        onChange={(pin) => {
          picks.push(pin)
          setValue(pin)
        }}
        models={value.host === "box" ? boxCatalog : CATALOG}
      />
    )
  }
  renderScreen(<Wrapper />)
  return picks
}

it("shows a host column left of the agents, and the host on the trigger, while hosts are listed", async () => {
  const user = userEvent.setup()
  stubIcons()
  renderHostPicker()

  expect(reads()).toMatch(/^local\s*Choose a model$/)
  await user.click(trigger())
  const hosts = await screen.findByRole("listbox", { name: "Hosts" })
  const agents = screen.getByRole("listbox", { name: "Agents" })
  expect(hosts.compareDocumentPosition(agents) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  expect(within(hosts).getAllByRole("option").map(head)).toEqual(["local", "box"])
  expect(selected("Agents")).toEqual(["codex-acp"])
})

it("disables a host the app cannot reach and shows its reason", async () => {
  const user = userEvent.setup()
  stubIcons()
  const picks = renderHostPicker({ reason: "no url" })

  await user.click(trigger())
  const row = within(await screen.findByRole("listbox", { name: "Hosts" })).getByRole("option", {
    name: /box/,
  })
  expect(row.getAttribute("aria-disabled")).toBe("true")
  expect(row.textContent).toContain("no url")
  await user.click(row)
  expect(picks).toEqual([])
})

it("fills the agent and model columns with the models of the picked host, and yields the host", async () => {
  const user = userEvent.setup()
  stubIcons()
  const picks = renderHostPicker()

  await user.click(trigger())
  await user.click(within(await screen.findByRole("listbox", { name: "Hosts" })).getByText("box"))

  expect(picks).toEqual([{ host: "box", model: "", effort: "" }])
  await waitFor(() => expect(heads("Agents")).toEqual(["opencode-acp"]))
  await user.click(option("Agents", "opencode-acp"))
  expect(heads("Models")).toEqual(["opencode-acp:box-model"])
  await user.click(option("Models", "opencode-acp:box-model"))
  expect(picks.at(-1)).toMatchObject({ host: "box", model: "opencode-acp:box-model" })
})

it("searches only the models of the selected host", async () => {
  const user = userEvent.setup()
  stubIcons()
  renderHostPicker({ start: "box" })

  await user.click(trigger())
  await screen.findByRole("listbox", { name: "Hosts" })
  await search(user, "claude")
  expect(screen.getByText("Nothing in the catalog matches.")).toBeDefined()
  await user.clear(screen.getByRole("searchbox", { name: "Search models" }))
  await search(user, "box")
  expect(heads("Models")).toEqual(["opencode-acp:box-model"])
})

it("shows no host column and no host on the trigger without a host list", async () => {
  const user = userEvent.setup()
  stubIcons()
  renderPicker()

  await openPicker(user)
  expect(screen.queryByRole("listbox", { name: "Hosts" })).toBeNull()
  expect(reads()).toBe("Choose a model")
})
