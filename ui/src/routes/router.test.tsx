// @vitest-environment jsdom

/**
 * The route table itself, through the app's own hash router: the addresses
 * that must lead somewhere, and the ones that must not any more.
 */

import { screen } from "@testing-library/react"
import { RouterProvider } from "react-router-dom"
import { expect, it } from "vitest"

import { stubEventSource } from "@/test/event-source"
import { renderScreen } from "@/test/harness"

import { router } from "./router"

async function open(hash: string) {
  stubEventSource()
  await router.navigate(hash)
  renderScreen(<RouterProvider router={router} />, { route: null })
}

it("mounts the repositories screen at #/repositories", async () => {
  await open("/repositories")

  expect(await screen.findByRole("heading", { name: "Repositories", level: 1 })).toBeDefined()
})

it("leads nowhere from a screen the app no longer has", async () => {
  await open("/graphs")

  expect(await screen.findByText("Nothing here")).toBeDefined()
})
