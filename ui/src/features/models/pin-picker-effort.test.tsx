// @vitest-environment jsdom

/**
 * The pin picker's effort rules and its keyboard: an effort never outlives a model
 * that cannot be run at it, and the columns are walked with the keys.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import { renderScreen } from "@/test/harness"
import {
  CATALOG,
  heads,
  LABEL,
  openPicker,
  option,
  popup,
  reads,
  renderPicker,
  resetPickerSettings,
  search,
  selected,
} from "@/test/pin-picker-kit"
import { PinPicker } from "./pin-picker"

beforeEach(resetPickerSettings)

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
          host="local"
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
