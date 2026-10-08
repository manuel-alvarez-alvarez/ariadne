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
    "failed_checks",
    "behind_base",
    "session_id",
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

it("links each failed check to its run and the session to its own panel", async () => {
  daemonFetch.mockImplementation(async () =>
    jsonResponse({
      ...pull,
      behind_base: true,
      session_id: "01JSESS00000000000000PULL1",
      failed_checks: [
        { name: "lint", url: "https://ci.example/lint", conclusion: "failure" },
        { name: "deploy", url: "", conclusion: "cancelled" },
      ],
    }),
  )
  const { location } = renderScreen(<PullRequestPanel id="pull-42" onClose={() => {}} />, {
    route: "/pull-requests?pr=pull-42",
  })
  const lint = await screen.findByRole("link", { name: "lint" })
  expect(lint.getAttribute("href")).toBe("https://ci.example/lint")
  expect(lint.getAttribute("target")).toBe("_blank")
  expect(screen.getByText("(failure)")).toBeTruthy()
  // A check the forge names no run for is its name alone.
  expect(screen.getByText("deploy")).toBeTruthy()
  expect(screen.queryByRole("link", { name: "deploy" })).toBeNull()
  expect(screen.getByText("behind_base").nextElementSibling?.textContent).toBe("true")
  await userEvent.click(screen.getByRole("link", { name: "01JSESS00000000000000PULL1" }))
  expect(location.url).toBe("/pull-requests?session=01JSESS00000000000000PULL1")
})
