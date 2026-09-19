// @vitest-environment jsdom

/**
 * The route table itself, through the app's own hash router: the addresses
 * that must lead somewhere, and the ones that must not any more.
 */

import { screen } from "@testing-library/react"
import { RouterProvider } from "react-router-dom"
import { expect, it, vi } from "vitest"

import { stubEventSource } from "@/test/event-source"
import { renderScreen } from "@/test/harness"

import { router } from "./router"

// jsdom has no WebGL, and sigma.js reads `WebGLRenderingContext` as its module
// loads: the graph is drawn by the stand-in in `@/test/sigma-canvas.tsx`. It is
// mocked here, in the files that draw a graph, and not in `@/test/setup`: a
// mock in the setup file slows every test file in the suite.
vi.mock("@/features/knowledge/graph/sigma-canvas", () => import("@/test/sigma-canvas"))

async function open(hash: string) {
  stubEventSource()
  await router.navigate(hash)
  renderScreen(<RouterProvider router={router} />, { route: null })
}

it("mounts the knowledge screen at #/knowledge", async () => {
  await open("/knowledge")

  expect(await screen.findByRole("heading", { name: "Knowledge", level: 1 })).toBeDefined()
})

it("leads nowhere from a repository's old knowledge page", async () => {
  await open("/repositories/01JREPO00000000000000ARI/knowledge")

  expect(await screen.findByText("Nothing here")).toBeDefined()
})
