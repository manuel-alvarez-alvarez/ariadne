// @vitest-environment jsdom

/**
 * What a person typed, drawn as a message: the shared prompt box of the turn
 * console.
 */

import { render, screen } from "@testing-library/react"
import { expect, it } from "vitest"

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

it("renders developer prompts as full Markdown without a fold", () => {
  const developer = { text: "## Resume\n- one", role: "Developer" }
  const { container } = render(<PromptBox {...developer} />)

  expect(container.querySelector("h2")?.textContent).toBe("Resume")
  expect(screen.queryByRole("button")).toBeNull()
  screen.getByText("Developer")
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
