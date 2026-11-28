// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, renderHook } from "@testing-library/react"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { expect, it } from "vitest"
import { HostPicker } from "@/components/host-picker"
import { HostSync } from "@/components/host-sync"
import { CreateGoalDialog } from "@/features/goals/create-goal-dialog"
import { HostsPage } from "@/features/hosts/hosts-page"
import { PermissionsPage } from "@/features/permissions/permissions-page"
import { RepositoriesPage } from "@/features/repositories/repositories-page"
import { SkillsPage } from "@/features/skills/skills-page"
import { CreateStandaloneTaskDialog } from "@/features/tasks/create-standalone-task-dialog"
import { WorkflowsPage } from "@/features/workflows/workflows-page"
import { useSettingsStore } from "@/stores/settings"
import { stubHosts } from "@/test/hosts"
import { useHostColor } from "./use-host-color"
import { useHostSelection, usePickedHost } from "./use-host-filter"

function wrapper({ children }: { children: React.ReactNode }) {
  return <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>
}

it("keeps All unresolved until the hosts answer", () => {
  useSettingsStore.setState({ hostFilter: "" })
  stubHosts({ local: { "/v1/hosts": () => undefined } })
  const { result } = renderHook(
    () => ({ selection: useHostSelection(), picked: usePickedHost() }),
    { wrapper },
  )
  expect(result.current.selection).toMatchObject({ isPending: true, read: [] })
  expect(result.current.picked).toBeNull()
})

it("shows skeletons for the host picker and catalog sync until hosts answer", () => {
  stubHosts({ local: { "/v1/hosts": () => undefined } })
  const { container } = render(
    <>
      <HostPicker />
      <HostSync />
    </>,
    { wrapper },
  )
  expect(container.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(2)
  expect(container.textContent).not.toContain("local")
})

it("shows no host colour before the hosts answer", () => {
  stubHosts({ local: { "/v1/hosts": () => undefined } })
  const { result } = renderHook(() => useHostColor("local"), { wrapper })
  expect(result.current).toBeNull()
})

it.each([
  ["hosts", HostsPage, "Loading hosts"],
  ["repositories", RepositoriesPage, "Loading host sync"],
  ["permissions", PermissionsPage, "Loading host sync"],
  ["skills", SkillsPage, "Loading host sync"],
  ["workflows", WorkflowsPage, "Loading host sync"],
])("keeps the %s screen unresolved while hosts load", (_name, Page, label) => {
  stubHosts({ local: { "/v1/hosts": () => undefined } })
  const router = createMemoryRouter([{ path: "/", element: <Page /> }])
  const { container } = render(<RouterProvider router={router} />, { wrapper })
  expect(container.querySelector(`[aria-label="${label}"][data-slot="skeleton"]`)).toBeTruthy()
  if (_name === "hosts") expect(container.querySelectorAll("tbody tr")).toHaveLength(3)
})

it.each([
  ["goal", CreateGoalDialog],
  ["task", CreateStandaloneTaskDialog],
])("keeps the %s form host unresolved while hosts load", (_name, Dialog) => {
  stubHosts({ local: { "/v1/hosts": () => undefined } })
  const { baseElement } = render(<Dialog open onOpenChange={() => {}} />, { wrapper })
  expect(
    baseElement.querySelector('[aria-label="Loading hosts"][data-slot="skeleton"]'),
  ).toBeTruthy()
})
