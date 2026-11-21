// @vitest-environment jsdom
import { fireEvent, screen } from "@testing-library/react"
import { renderScreen } from "@/test/harness"

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
it("keeps URL filters visible when their last item disappears", () => {
  renderScreen(<AttentionPage />, {
    route: "/attention?attentionGoal=gone&attentionReason=task:failed&attentionHost=remote",
  })
  expect((screen.getByLabelText("Goal") as HTMLSelectElement).value).toBe("gone")
  expect((screen.getByLabelText("Reason") as HTMLSelectElement).value).toBe("task:failed")
  expect((screen.getByLabelText("Host") as HTMLSelectElement).value).toBe("remote")
  expect(screen.getByText("No attention items match these filters.")).toBeTruthy()
  fireEvent.change(screen.getByLabelText("Host"), { target: { value: "" } })
  expect((screen.getByLabelText("Host") as HTMLSelectElement).value).toBe("")
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
