// @vitest-environment jsdom

import { render, screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"

import { PrLink } from "./pr-link"

function mount(props: Parameters<typeof PrLink>[0]) {
  return render(
    <TooltipProvider>
      <PrLink {...props} />
    </TooltipProvider>,
  )
}

it("shows a compact number and keeps the full destination", () => {
  mount({ url: "https://github.com/acme/app/pull/244", context: "Fix it" })

  const link = screen.getByRole("link", { name: "Pull request #244 in acme/app: Fix it" })
  expect(link.textContent).toBe("#244")
  expect(link.getAttribute("href")).toBe("https://github.com/acme/app/pull/244")
})

it("falls back to a compact label when the number is unknown", () => {
  mount({ url: "https://example.com/somewhere" })

  const link = screen.getByRole("link", { name: "Pull request" })
  expect(link.textContent).toBe("PR")
})
