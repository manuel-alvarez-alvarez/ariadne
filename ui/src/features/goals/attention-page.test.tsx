// @vitest-environment jsdom
import { act, fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { dispatchDomainEvent } from "@/events/dispatch"
import { renderScreen } from "@/test/harness"
import { BOX, LOCAL, stubHosts } from "@/test/hosts"

vi.mock("@/api", async (original) => ({
  ...(await original<typeof import("@/api")>()),
  useHostLists: () => ({ rows: [] }),
}))
vi.mock("@/hooks/use-host-filter", async (original) => ({
  ...(await original<typeof import("@/hooks/use-host-filter")>()),
  useReachableHosts: () => ["local"],
}))

import { expect, it, vi } from "vitest"
import type { AttentionItem } from "./attention"
import { AttentionPage } from "./attention-page"

let items: AttentionItem[] = []
vi.mock("./attention", async (original) => ({
  ...(await original<typeof import("./attention")>()),
  useAttention: () => ({ items, errors: [], error: null, partial: false, isPending: false }),
}))
it("keeps URL filters visible when their last item disappears", async () => {
  const user = userEvent.setup()
  renderScreen(<AttentionPage />, {
    route: "/attention?attentionGoal=gone&attentionReason=task:failed&attentionHost=remote",
  })
  expect(screen.getByRole("combobox", { name: "Goal" }).textContent).toContain("gone")
  expect(screen.getByRole("combobox", { name: "Reason" }).textContent).toContain("task:failed")
  expect(screen.getByRole("combobox", { name: "Host" }).textContent).toContain("remote")
  expect(screen.getByText("No attention items match these filters.")).toBeTruthy()
  await user.click(screen.getByRole("combobox", { name: "Host" }))
  await user.click(await screen.findByRole("option", { name: "All hosts" }))
  expect(screen.getByRole("combobox", { name: "Host" }).textContent).toContain("All hosts")
})

it("keeps a host named __all__ apart from All in the host filter, in colour", async () => {
  const user = userEvent.setup()
  const odd = { ...BOX, name: "__all__", color: "violet" as const }
  stubHosts({}, [LOCAL, odd])
  items = [{ ...item("a"), host: "__all__" }]
  const { queryClient, location } = renderScreen(<AttentionPage />, { route: "/attention" })
  const select = screen.getByRole("combobox", { name: "Host" })
  await user.click(select)
  await user.click(await screen.findByRole("option", { name: "__all__" }))
  expect(location.url).toContain("attentionHost=__all__")
  const hue = () => select.querySelector("[data-host-color]")?.getAttribute("data-host-color")
  await waitFor(() => expect(hue()).toBe("violet"))
  act(() => {
    dispatchDomainEvent(queryClient, "local", {
      event: "host_updated",
      data: { ...odd, color: "pink" },
    })
  })
  await waitFor(() => expect(hue()).toBe("pink"))
  await user.click(select)
  await user.click(await screen.findByRole("option", { name: "All hosts" }))
  expect(location.url).not.toContain("attentionHost")
  items = []
})

it("draws the hosts of the host filter with their colour, and repaints it live", async () => {
  const user = userEvent.setup()
  stubHosts({}, [LOCAL, { ...BOX, color: "teal" }])
  items = [{ ...item("a"), host: "box" }]
  const { queryClient } = renderScreen(<AttentionPage />, {
    route: "/attention?attentionHost=box",
  })
  const select = screen.getByRole("combobox", { name: "Host" })
  const hue = () => select.querySelector("[data-host-color]")?.getAttribute("data-host-color")
  await waitFor(() => expect(hue()).toBe("teal"))
  await user.click(select)
  const options = await screen.findAllByRole("option")
  expect(
    options.map((o) => o.querySelector("[data-host-color]")?.getAttribute("data-host-color")),
  ).toEqual([undefined, "teal"])
  await user.keyboard("{Escape}")
  act(() => {
    dispatchDomainEvent(queryClient, "local", {
      event: "host_updated",
      data: { ...BOX, color: "pink" },
    })
  })
  await waitFor(() => expect(hue()).toBe("pink"))
  items = []
})

function item(id: string): AttentionItem {
  return {
    host: "local",
    id,
    goalId: "",
    goal: undefined,
    at: "2026-11-19T00:00:00Z",
    taskId: null,
    task: undefined,
    taskReason: null,
    session: undefined,
    sessionReason: null,
    recovery: {
      id,
      occurrence: "",
      producer: "recovery",
      reason: "resource",
      summary: id,
      required_action: "Free resources",
      since: "2026-11-19T00:00:00Z",
      affected: [],
      target: { kind: "task", task_id: id },
    },
  }
}
it("preserves selected position when new items arrive and explains withdrawal", () => {
  items = [item("first"), item("second")]
  const view = renderScreen(<AttentionPage />, { route: "/attention" })
  fireEvent.click(screen.getByRole("button", { name: /firstFree resources/ }))
  expect(view.location.url).toContain("attentionItem=local%0Afirst")
  items = [item("new"), item("first"), item("second")]
  view.rerender(<AttentionPage />)
  fireEvent.click(screen.getByRole("button", { name: "Next" }))
  expect(view.location.url).toContain("attentionItem=local%0Asecond")
  items = [item("new"), item("first")]
  view.rerender(<AttentionPage />)
  expect(screen.getByRole("status").textContent).toContain(
    "Selected item resolved or is unavailable",
  )
  items = []
})
