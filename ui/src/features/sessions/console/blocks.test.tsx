// @vitest-environment jsdom

/**
 * What a person typed, drawn as a message: the shared prompt box of the turn
 * console.
 */

import { fireEvent, render, screen } from "@testing-library/react"
import { expect, it, vi } from "vitest"

import { PromptBox } from "./blocks"

it("renders a prompt as Markdown: headings, task lists, a fenced block, and a single newline still breaks", () => {
  const text = "# Plan\n- [x] done\n- [ ] todo\n\nline one\nline two\n\n```ts\nconst a = 1\n```"
  const { container } = render(<PromptBox text={text} />)

  expect(container.querySelector("h1")?.textContent).toBe("Plan")
  expect(container.querySelectorAll("input[type=checkbox]")).toHaveLength(2)
  expect(container.querySelector("[data-code-block] code")?.textContent).toBe("const a = 1")
  const paragraph = container.querySelector("p")
  expect(paragraph?.textContent).toBe("line one\nline two")
  expect(paragraph?.parentElement?.className).toContain("[&_p]:whitespace-pre-wrap")
})

it("renders a developer prompt as Markdown, folded to 4 lines with Show more where it is longer", () => {
  const onToggle = vi.fn()
  const developer = { role: "Developer" }
  const long = "## Resume\n- one\n- two\n- three\n- four"
  const view = render(<PromptBox text={long} fold={{ open: false, onToggle }} {...developer} />)

  expect(view.container.querySelector("h2")).toBeNull()
  expect(view.container.textContent).toContain("- three")
  expect(view.container.textContent).not.toContain("- four")
  fireEvent.click(screen.getByRole("button", { name: "Show more" }))
  expect(onToggle).toHaveBeenCalledOnce()

  view.rerender(<PromptBox text={long} fold={{ open: true, onToggle }} {...developer} />)
  expect(view.container.querySelector("h2")?.textContent).toBe("Resume")
  screen.getByRole("button", { name: "Show less" })
  screen.getByRole("img", { name: "Developer" })

  view.rerender(<PromptBox text="## Short" fold={{ open: false, onToggle }} {...developer} />)
  expect(screen.queryByRole("button")).toBeNull()
})

it("draws <kbd> in a prompt as a kbd element, and leaves other raw HTML as text", () => {
  const { container } = render(<PromptBox text="Press <kbd>Ctrl</kbd> and <b>x</b>" />)

  expect(container.querySelector("kbd")?.textContent).toBe("Ctrl")
  expect(container.querySelector("b")).toBeNull()
})

it("shows the time of a prompt and marks a queued one", () => {
  const { container } = render(<PromptBox text="Fix it" time="08:30:15" queued />)

  expect(container.querySelector("[data-block-time]")?.textContent).toBe("08:30:15")
  screen.getByText("queued")
})
