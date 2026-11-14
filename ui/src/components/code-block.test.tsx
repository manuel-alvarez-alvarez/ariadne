// @vitest-environment jsdom

import { render, waitFor } from "@testing-library/react"
import { expect, it } from "vitest"

import { CodeBlock } from "./code-block"

const code = (container: HTMLElement) => container.querySelector("code")

it("shows the plain text first and the same text highlighted once the grammar loads", async () => {
  const text = `def f():\n    return """one\ntwo"""\n`
  const { container } = render(<CodeBlock code={text} language="py" />)

  expect(code(container)?.querySelector("span")).toBeNull()
  expect(code(container)?.textContent).toBe(text)

  await waitFor(() => expect(code(container)?.querySelector("span")).not.toBeNull())
  expect(code(container)?.textContent).toBe(text)
  const string = Array.from(code(container)?.querySelectorAll("span") ?? []).find(
    (span) => span.textContent === `"""one\ntwo"""`,
  )
  expect(string?.className).toBeTruthy()
  expect(container.querySelector("[data-code-language]")?.textContent).toBe("Python")
})

it("shows a language the data does not know as plain text with no label", () => {
  const { container } = render(<CodeBlock code="fn main() {}" language="mystery" />)

  expect(code(container)?.querySelector("span")).toBeNull()
  expect(code(container)?.textContent).toBe("fn main() {}")
  expect(container.querySelector("[data-code-language]")?.textContent).toBe("")
})

it("keeps the tints of a diff's added, removed and hunk-header lines", () => {
  const { container } = render(<CodeBlock code={"@@ -1 +1 @@\n-old\n+new\n same"} diff />)
  const lines = Array.from(code(container)?.children ?? []).map((line) => line.className)

  expect(lines).toEqual([
    "text-muted-foreground",
    "text-status-danger-fg",
    "text-status-done-fg",
    "",
  ])
})

it("shows a fence named after an object property as plain text", () => {
  for (const language of ["constructor", "__proto__"]) {
    const { container, unmount } = render(<CodeBlock code="x" language={language} />)
    expect(code(container)?.textContent).toBe("x")
    expect(container.querySelector("[data-code-language]")?.textContent).toBe("")
    unmount()
  }
})
