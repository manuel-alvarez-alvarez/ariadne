// @vitest-environment jsdom

import { render, waitFor } from "@testing-library/react"
import { toast } from "sonner"
import { expect, it } from "vitest"

import { Toaster } from "./sonner"

it("renders toasts at the bottom left", async () => {
  render(<Toaster />)
  toast("Toast")

  await waitFor(() => {
    const toaster = document.querySelector("[data-sonner-toaster]")
    expect(toaster?.getAttribute("data-y-position")).toBe("bottom")
    expect(toaster?.getAttribute("data-x-position")).toBe("left")
  })
})
