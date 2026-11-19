// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from "@testing-library/react"
import { expect, it, vi } from "vitest"
import type { AttentionItemDto } from "@/api"
import { DetailPanels } from "@/components/detail-panels"
import { renderScreen } from "@/test/harness"
import { Row } from "./attention-strip"

vi.mock("@/features/goals/goal-panel", () => ({
  GoalPanel: ({ onClose }: { onClose: () => void }) => (
    <button type="button" onClick={onClose}>
      Close detail
    </button>
  ),
}))
vi.mock("@/features/pull-requests/pull-request-panel", () => ({
  PullRequestPanel: ({ onClose }: { onClose: () => void }) => (
    <button type="button" onClick={onClose}>
      Close detail
    </button>
  ),
}))
vi.mock("@/features/sessions/session-panel", () => ({
  SessionPanel: ({ onClose }: { onClose: () => void }) => (
    <button type="button" onClick={onClose}>
      Close detail
    </button>
  ),
}))
vi.mock("@/features/tasks", async (original) => ({
  ...(await original<typeof import("@/features/tasks")>()),
  TaskPanel: ({ onClose }: { onClose: () => void }) => (
    <button type="button" onClick={onClose}>
      Close detail
    </button>
  ),
}))

it.each<AttentionItemDto["target"]>([
  { kind: "task", task_id: "task1" },
  { kind: "console", session_id: "session1" },
  { kind: "goal", goal_id: "goal1" },
  { kind: "pull_request", pull_request_id: "pr1" },
])("returns to the preserved attention queue after opening $kind", async (target) => {
  const recovery: AttentionItemDto = {
    id: "selected",
    occurrence: "",
    producer: "recovery",
    reason: "resource",
    summary: "Read this item",
    required_action: "Free resources",
    since: "2026-11-19T00:00:00Z",
    affected: [],
    target,
  }
  const route =
    "/attention?attentionGoal=goal1&attentionReason=recovery%3Aresource&attentionHost=local&attentionItem=selected"
  const view = renderScreen(
    <>
      <ul>
        <Row
          item={{
            host: "local",
            id: "selected",
            goalId: "",
            goal: undefined,
            at: recovery.since,
            taskId: null,
            task: undefined,
            taskReason: null,
            session: undefined,
            sessionReason: null,
            recovery,
          }}
          expanded
        />
      </ul>
      <DetailPanels />
    </>,
    { route },
  )
  fireEvent.click(screen.getByRole("link", { name: /Read this item/ }))
  expect(view.location.url.split("?")[0]).toBe("/attention")
  for (const [key, value] of new URLSearchParams(route.split("?")[1]))
    expect(new URLSearchParams(view.location.url.split("?")[1]).get(key)).toBe(value)
  fireEvent.click(screen.getByRole("button", { name: "Close detail" }))
  await waitFor(() => expect(view.location.url).toBe(route))
})
