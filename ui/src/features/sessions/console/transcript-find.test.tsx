// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react"
import { expect, it } from "vitest"
import { TranscriptFind } from "./transcript-find"

it("finds folded text, keeps the selected occurrence on updates, and returns focus", () => {
  const props = {
    entries: [{ id: "a", text: "hidden timeout and timeout" }],
    older: true,
    loading: false,
    loadOlder: () => {},
  }
  const { rerender } = render(
    <div data-console>
      <TranscriptFind {...props} />
    </div>,
  )
  const trigger = screen.getByRole("button", { name: "Find in transcript" })
  fireEvent.click(trigger)
  fireEvent.change(screen.getByRole("textbox", { name: "Find in transcript" }), {
    target: { value: "timeout" },
  })
  expect(screen.getByRole("status").textContent).toContain("1 of 2")
  fireEvent.click(screen.getByRole("button", { name: "Next match" }))
  rerender(
    <div data-console>
      <TranscriptFind {...props} entries={[{ id: "before", text: "timeout" }, ...props.entries]} />
    </div>,
  )
  expect(screen.getByRole("status").textContent).toContain("3 of 3")
  expect(screen.getByText(/Older history remains unloaded/)).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Close find" }))
  expect(document.activeElement).toBe(trigger)
})

it("scopes the shortcut and preserves text field ownership", () => {
  render(
    <div data-console>
      <button type="button">Console control</button>
      <textarea aria-label="Draft" defaultValue="unsent" />
      <TranscriptFind entries={[]} older={false} loading={false} loadOlder={() => {}} />
    </div>,
  )
  const draft = screen.getByRole("textbox", { name: "Draft" })
  fireEvent.keyDown(draft, { key: "f", ctrlKey: true })
  expect(screen.queryByRole("textbox", { name: "Find in transcript" })).toBeNull()
  fireEvent.keyDown(screen.getByText("Console control"), { key: "f", metaKey: true })
  expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Find in transcript" }))
  expect((draft as HTMLTextAreaElement).value).toBe("unsent")
  fireEvent.keyDown(document.activeElement as Element, { key: "Escape" })
  expect(document.activeElement).toBe(screen.getByText("Console control"))
})
