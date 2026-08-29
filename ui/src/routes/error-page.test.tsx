// @vitest-environment jsdom

/**
 * The two ways a screen can fail to be there, which used to be one screen.
 *
 * A URL that resolves to nothing is a wrong address: the words are "Nothing
 * here" and the way on is a link to somewhere that exists. A component that
 * throws is a crash: the words are "Something went wrong", the way on is a
 * reload, and what it said has to be something a person can hand to whoever
 * will fix it. Announcing the second as the first — which is what one screen
 * for both did — left a crash looking like a typo in the address bar.
 *
 * Rendered through a real router, because `useRouteError` is only meaningful
 * inside one and the branch under test is what the router hands it.
 */

import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { expect, it, vi } from "vitest"

import { RouteErrorPage } from "./error-page"
import { NotFoundPage } from "./not-found-page"

/** A screen that throws on render, which is the failure the boundary is for. */
function Boom(): never {
  throw new Error("Cannot read properties of undefined (reading 'title')")
}

function mount(element: React.ReactElement, path = "/boom") {
  const router = createMemoryRouter(
    [
      { path: "/boom", element, errorElement: <RouteErrorPage /> },
      // The router's own way of saying a thing is not there: a route that
      // throws a `Response`, which is what `isRouteErrorResponse` recognises.
      {
        path: "/missing",
        loader: () => {
          throw new Response("no such goal", { status: 404, statusText: "Not Found" })
        },
        element: <div />,
        errorElement: <RouteErrorPage />,
      },
      { path: "*", element: <NotFoundPage /> },
    ],
    { initialEntries: [path] },
  )
  render(<RouterProvider router={router} />)
}

it("says a crash is a crash, and shows what it said", () => {
  // React logs the boundary's catch; the test is about the screen, not the log.
  const error = vi.spyOn(console, "error").mockImplementation(() => {})
  mount(<Boom />)

  expect(screen.getByRole("heading", { name: "Something went wrong" })).toBeDefined()
  expect(screen.getByText(/Cannot read properties of undefined/)).toBeDefined()
  expect(screen.getByRole("button", { name: "Reload" })).toBeDefined()
  expect(screen.getByRole("link", { name: "Back to goals" }).getAttribute("href")).toBe("/goals")
  error.mockRestore()
})

it("hands the whole of it to the clipboard, address included", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {})
  const user = userEvent.setup()
  mount(<Boom />)

  await user.click(screen.getByRole("button", { name: "Copy details" }))

  const copied = await navigator.clipboard.readText()
  expect(copied).toContain("Cannot read properties of undefined")
  expect(copied).toContain(window.location.href)
  error.mockRestore()
})

it("gives a router-thrown 404 the words for a wrong address, and no stack", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {})
  mount(<div />, "/missing")

  expect(await screen.findByRole("heading", { name: "Nothing here" })).toBeDefined()
  expect(screen.getByText("This page does not exist.")).toBeDefined()
  expect(screen.queryByRole("button", { name: "Copy details" })).toBeNull()
  error.mockRestore()
})

it("says the same for a URL that matches no route at all", () => {
  mount(<Boom />, "/nowhere")

  expect(screen.getByRole("heading", { name: "Nothing here" })).toBeDefined()
  // Nothing threw, so there is nothing to report — and nothing to reload.
  expect(screen.queryByRole("button", { name: "Reload" })).toBeNull()
})
