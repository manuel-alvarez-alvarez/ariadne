// @vitest-environment jsdom

import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"
import { Markdown } from "./markdown"

it("marks untrusted prose while preserving Markdown and code examples", () => {
  render(
    <Markdown>
      {"Before <<<untrusted>>>**unsafe** text<<<end untrusted>>> after `<<<untrusted>>>`"}
    </Markdown>,
  )
  expect(screen.getByText("unsafe").closest("mark")?.getAttribute("title")).toBe(
    "Untrusted content",
  )
  expect(screen.getByText("unsafe").parentElement?.tagName).toBe("STRONG")
  expect(screen.getByText("<<<untrusted>>>").tagName).toBe("CODE")
  expect(screen.queryByText("<<<end untrusted>>>")).toBeNull()
})

it("updates an open untrusted span as text streams and keeps code markers literal", () => {
  const view = render(<Markdown>{"<<<untrusted>>>first"}</Markdown>)
  expect(view.container.querySelector("mark")?.textContent).toBe("first")
  view.rerender(
    <Markdown>
      {"<<<untrusted>>>first **second**<<<end untrusted>>> safe\n\n```\n<<<untrusted>>>\n```"}
    </Markdown>,
  )
  expect(view.container.querySelector("strong mark")?.textContent).toBe("second")
  expect(view.container.querySelector("[data-code-block] code")?.textContent).toBe(
    "<<<untrusted>>>",
  )
  expect(screen.getByText("safe").closest("mark")).toBeNull()
})

it("offers the trust tooltip on keyboard focus", async () => {
  const { container } = render(<Markdown>{"<<<untrusted>>>unsafe<<<end untrusted>>>"}</Markdown>)
  const mark = container.querySelector("mark")
  expect(mark?.tabIndex).toBe(0)
  await userEvent.tab()
  expect((await screen.findByText("Untrusted content")).textContent).toBe("Untrusted content")
})
