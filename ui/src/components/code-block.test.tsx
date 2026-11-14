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

it.each([
  { firstLine: 1, digits: 1 },
  { firstLine: 100, digits: 3 },
  { firstLine: 10000, digits: 5 },
])(
  "sizes the line-number gutter for $digits-digit numbers plus its right padding",
  ({ firstLine, digits }) => {
    const { container } = render(<CodeBlock code="a" firstLine={firstLine} />)
    const row = container.querySelector("[data-line]") as HTMLElement

    expect(row.style.getPropertyValue("--gutter")).toBe(`calc(${digits}ch + 0.75rem)`)
  },
)

/** Whether a class list sets a border or a background of its own, not on hover or focus. */
const framed = (node: Element) =>
  /(^|\s)(border|border-[blrtxy]|bg-[\w/[\]-]+)(\s|$)/.test(node.getAttribute("class") ?? "")

it("draws a fence in one light frame, its label and an icon-only Copy on one caption line", () => {
  const { container, getByRole } = render(<CodeBlock code="let x = 1" language="ts" />)
  const block = container.querySelector("[data-code-block]") as HTMLElement

  expect(framed(block)).toBe(true)
  const inner = Array.from(block.querySelectorAll("*")).filter(
    (node) => node.tagName !== "BUTTON" && framed(node),
  )
  expect(inner).toEqual([])
  const copy = getByRole("button", { name: "Copy TypeScript code" })
  expect(copy.textContent).toBe("")
  expect(copy.closest("[data-code-caption]")?.textContent).toBe("TypeScript")
})

it("draws a captioned block bare, under a caption that names the section and the language", () => {
  const { container, getByRole } = render(
    <CodeBlock code='{"a": 1}' language="json" caption="output" />,
  )
  const nodes = [
    container.querySelector("[data-code-block]"),
    ...container.querySelectorAll("[data-code-block] *"),
  ]

  expect(nodes.filter((node) => node && node.tagName !== "BUTTON" && framed(node))).toEqual([])
  expect(container.querySelector("[data-code-caption]")?.textContent).toBe("output · JSON")
  expect(getByRole("button", { name: "Copy output" }).textContent).toBe("")
})

it("shows a fence named after an object property as plain text", () => {
  for (const language of ["constructor", "__proto__"]) {
    const { container, unmount } = render(<CodeBlock code="x" language={language} />)
    expect(code(container)?.textContent).toBe("x")
    expect(container.querySelector("[data-code-language]")?.textContent).toBe("")
    unmount()
  }
})
