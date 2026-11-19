// @vitest-environment jsdom

/** The note and the sync lines under the four screens `local` owns. */

import { screen } from "@testing-library/react"
import { createBrowserRouter, RouterProvider } from "react-router-dom"
import { afterEach, describe, expect, it } from "vitest"

import { PermissionsPage } from "@/features/permissions/permissions-page"
import { RepositoriesPage } from "@/features/repositories/repositories-page"
import { SkillsPage } from "@/features/skills/skills-page"
import { WorkflowsPage } from "@/features/workflows/workflows-page"
import { renderScreen, startAt } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"

afterEach(() => forgetHosts())

const SCREENS = [
  ["skills", SkillsPage],
  ["workflows", WorkflowsPage],
  ["permissions", PermissionsPage],
  ["repositories", RepositoriesPage],
] as const

function renderOnRouter(Screen: () => React.ReactNode) {
  startAt("/")
  const router = createBrowserRouter([{ path: "/", element: <Screen /> }])
  return renderScreen(<RouterProvider router={router} />, { route: null })
}

const ANSWERS = {
  "/v1/skills": [],
  "/v1/workflows": [],
  "/v1/permissions/learned": { items: [] },
  "/v1/repositories": [],
}

describe.each(SCREENS)("the %s screen", (name, Screen) => {
  it("says it shows and writes local, and heads the sync lines of the named hosts", async () => {
    stubHosts({ local: ANSWERS }, [LOCAL, BOX])
    renderOnRouter(Screen)

    expect(await screen.findByText(/shows and writes the settings of/)).toBeTruthy()
    expect(screen.getByTestId("local-note").textContent).toContain("local")
    expect(screen.getByRole("heading", { name: "Sync to other hosts" })).toBeTruthy()
    expect(screen.getByTestId("sync-box")).toBeTruthy()
  })

  it("shows no note, heading or sync line with only local listed", async () => {
    stubHosts({ local: ANSWERS }, [LOCAL])
    renderOnRouter(Screen)

    // Each screen's own control is up once it has mounted; the hosts answer in the
    // same tick as the stub, so a note that was going to show has shown by then.
    await screen.findAllByRole(name === "permissions" ? "tab" : "button")
    expect(screen.queryByTestId("local-note")).toBeNull()
    expect(screen.queryByRole("heading", { name: "Sync to other hosts" })).toBeNull()
    expect(screen.queryByRole("list", { name: "Sync" })).toBeNull()
    expect(screen.queryByRole("columnheader", { name: "Hosts" })).toBeNull()
    expect(screen.queryByRole("list", { name: "Checkouts" })).toBeNull()
  })
})

it("reads a pending sync of a host the app cannot reach as waiting for host", async () => {
  const pending = {
    state: "pending" as const,
    error: null,
    missing_repositories: [],
    skipped_permissions: 0,
  }
  stubHosts({ local: ANSWERS }, [
    LOCAL,
    { ...BOX, sync: pending },
    {
      ...BOX,
      name: "far",
      kind: "ssh",
      endpoint: null,
      status: "down",
      error: "no forward",
      sync: pending,
    },
  ])
  renderOnRouter(SkillsPage)

  expect((await screen.findByTestId("sync-box")).textContent).toContain("pending")
  expect(screen.getByTestId("sync-far").textContent).toContain("waiting for host")
  expect(screen.getByTestId("sync-far").textContent).not.toContain("pending")
})
