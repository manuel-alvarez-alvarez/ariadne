// @vitest-environment jsdom
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { pull } from "@/test/pull-request"
import { PullRequestPanel } from "./pull-request-panel"

it("shows every inspect field and removes a user row", async () => {
  const close = vi.fn()
  daemonFetch.mockImplementation(async (input) =>
    (input as Request).method === "DELETE"
      ? new Response(null, { status: 204 })
      : jsonResponse(pull),
  )
  renderScreen(<PullRequestPanel id="pull-42" onClose={close} />)
  await screen.findByText("head_sha")
  for (const label of [
    "id",
    "repository_id",
    "number",
    "url",
    "title",
    "author_login",
    "role",
    "tracked_by",
    "state",
    "draft",
    "head_branch",
    "head_sha",
    "head_repo",
    "base_branch",
    "checks",
    "review_decision",
    "unanswered_comments",
    "ready",
    "origin_task_id",
    "opened_at",
    "last_seen_at",
    "created_at",
    "updated_at",
  ]) {
    expect(screen.getByText(label)).toBeTruthy()
  }
  expect(screen.getByText("abc123")).toBeTruthy()
  expect(screen.getByText("task-1")).toBeTruthy()
  await userEvent.click(screen.getByRole("button", { name: "Remove" }))
  await waitFor(() => expect(close).toHaveBeenCalledOnce())
})
