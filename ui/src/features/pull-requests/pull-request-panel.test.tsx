// @vitest-environment jsdom

/**
 * The panel over one pull request: its facts read as a person reads them,
 * its description rendered, its sessions, and what can be done to it — an
 * Ariadne review of a request of the user's own, on the model and skills
 * they pick, and the removal of one added by hand.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import type { PullRequestDto, RepositoryDto } from "@/api"
import { aForge, aModel, aRepository, aSession, aSessionPage } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { pull } from "@/test/pull-request"
import { PullRequestPanel } from "./pull-request-panel"

const SKILLS = ["code-review", "debugging", "pr-reviewer", "orchestration"].map((name) => ({
  name,
  seat:
    name === "orchestration" ? "orchestrator" : name === "pr-reviewer" ? "pull_request" : "task",
  summary: `${name} summary`,
  builtin: true,
  document: "",
  document_is_default: true,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-01T00:00:00Z",
}))

/** A daemon answering the request as `row`, and recording every write. */
function daemon(
  row: () => PullRequestDto,
  writes: Request[] = [],
  repositories: RepositoryDto[] = [],
) {
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    if (request.method !== "GET") writes.push(request.clone())
    if (request.method === "DELETE") return new Response(null, { status: 204 })
    if (path === "/v1/models") return jsonResponse([aModel({ id: "stub:review-model" })])
    if (path === "/v1/skills") return jsonResponse(SKILLS)
    if (path === "/v1/sessions") return jsonResponse({ sessions: [], next_cursor: null })
    if (path === "/v1/repositories") return jsonResponse(repositories)
    return jsonResponse(row())
  })
}

it("reads its facts by name, renders its description, links the forge, and removes nothing", async () => {
  const asked: string[] = []
  daemon(() => ({ ...pull, body: "## Summary\n\n- Fixes **widgets** on an empty list." }))
  const answer = daemonFetch.getMockImplementation()
  daemonFetch.mockImplementation(async (input) => {
    asked.push(new URL((input as Request).url).pathname)
    return answer?.(input)
  })
  renderScreen(<PullRequestPanel id="repo:42" onClose={() => {}} />)

  await screen.findByText("#42 Fix widgets")
  // Read off the forge by its repository and number.
  expect(asked).toContain("/v1/repositories/repo/pull-requests/42")
  for (const label of [
    "Repository",
    "Author",
    "Branches",
    "Head",
    "Unanswered comments",
    "Ariadne",
  ]) {
    expect(screen.getByText(label)).toBeTruthy()
  }
  expect(screen.getByText("Reviews it")).toBeTruthy()
  expect(screen.queryByText("head_sha")).toBeNull()
  expect(screen.getByRole("heading", { name: "Summary" })).toBeTruthy()
  expect(screen.getByText("widgets").tagName).toBe("STRONG")
  const forge = screen.getByRole("link", { name: /#42 on the forge/ })
  expect(forge.getAttribute("href")).toBe(pull.url)
  // Ariadne lets go of a request on its own: nothing removes one by hand.
  expect(screen.queryByRole("button", { name: "Remove" })).toBeNull()
})

it("links each failed check to its run, and lists the request's review sessions", async () => {
  const asked: string[] = []
  daemon(() => ({
    ...pull,
    behind_base: true,
    failed_checks: [
      { name: "lint", url: "https://ci.example/lint", conclusion: "failure" },
      { name: "deploy", url: "", conclusion: "cancelled" },
    ],
  }))
  const fetch = daemonFetch.getMockImplementation()
  daemonFetch.mockImplementation(async (input) => {
    asked.push((input as Request).url)
    return fetch?.(input)
  })
  renderScreen(<PullRequestPanel id="repo:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo%3A42",
  })
  const lint = await screen.findByRole("link", { name: "lint" })
  expect(lint.getAttribute("href")).toBe("https://ci.example/lint")
  expect(screen.getByText("deploy")).toBeTruthy()
  expect(screen.queryByRole("link", { name: "deploy" })).toBeNull()
  expect(screen.getByText("Ahead of the head")).toBeTruthy()

  await userEvent.click(screen.getByRole("tab", { name: "Sessions" }))
  expect(await screen.findByText("No Ariadne review of this request yet")).toBeTruthy()
  expect(asked.some((url) => new URL(url).searchParams.get("pull_request") === "pull-42")).toBe(
    true,
  )
})

it("starts an Ariadne review of a request of mine on the model and skills picked, opens its console, and stops it", async () => {
  const user = userEvent.setup()
  let row: PullRequestDto = { ...pull, role: "author", author_login: "me" }
  const review = aSession({
    id: "01JSESS0000000000000REVIEW",
    seat: "reviewer",
    goal_id: null,
    task_id: null,
    pull_request_id: "pull-42",
  })
  const writes: Request[] = []
  daemon(() => row, writes)
  const update = daemonFetch.getMockImplementation()
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    if (request.method === "PUT") {
      const body = (await request.clone().json()) as { asked: boolean; skills?: string[] }
      row = { ...row, review_asked: body.asked, review_skills: body.skills ?? [] }
    }
    // The daemon starts the review session a moment after the ask.
    if (path === "/v1/sessions" && row.review_asked) return jsonResponse(aSessionPage([review]))
    if (path === `/v1/sessions/${review.id}`) return jsonResponse(review)
    return update?.(input)
  })
  const { location } = renderScreen(<PullRequestPanel id="repo:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo%3A42",
  })

  await user.click(await screen.findByRole("button", { name: "Start review" }))
  const dialog = await screen.findByRole("dialog", { name: "Start an Ariadne review" })
  const start = within(dialog).getByRole("button", { name: "Start review" })
  expect(start.hasAttribute("disabled")).toBe(true)

  await user.click(within(dialog).getByRole("button", { name: "Model" }))
  const models = await screen.findByRole("listbox", { name: "Models" })
  await user.click(within(models).getByText("stub:review-model"))
  await user.keyboard("{Escape}")
  const skills = within(dialog).getByRole("group", { name: "Skills" })
  const playbook = within(skills).getByRole("button", { name: "pr-reviewer" })
  expect(playbook.getAttribute("aria-pressed")).toBe("true")
  expect(playbook.hasAttribute("disabled")).toBe(true)
  // Nothing beside the playbook is picked to start with.
  expect(
    within(skills).getByRole("button", { name: "code-review" }).getAttribute("aria-pressed"),
  ).toBe("false")
  expect(within(skills).queryByRole("button", { name: "orchestration" })).toBeNull()
  await user.click(within(skills).getByRole("button", { name: "code-review" }))
  await user.click(within(skills).getByRole("button", { name: "debugging" }))
  await user.click(within(dialog).getByRole("button", { name: "Start review" }))

  await waitFor(() => expect(writes).toHaveLength(1))
  expect(await writes[0]?.json()).toEqual({
    asked: true,
    model: "stub:review-model",
    skills: ["code-review", "debugging"],
  })
  // The dialog closes, and the panel opens the review's session once it is up.
  await waitFor(() =>
    expect(screen.queryByRole("dialog", { name: "Start an Ariadne review" })).toBeNull(),
  )
  await waitFor(() => expect(location.url).toContain(`session=${review.id}`), { timeout: 4000 })
  expect(location.url).toContain("tab=sessions")

  // Back on the request, the review can be stopped. The session's header
  // replaces the one its loading showed, breadcrumb and all, so the click
  // is made on whichever one is on screen until it takes.
  await waitFor(async () => {
    const back = within(screen.getByRole("navigation", { name: "Breadcrumb" }))
    await user.click(back.getByRole("button", { name: /#42 Fix widgets/ }))
    expect(location.url).not.toContain("session=")
  })
  await user.click(await screen.findByRole("button", { name: "Stop review" }, { timeout: 4000 }))
  await waitFor(() => expect(writes).toHaveLength(2))
  expect(await writes[1]?.json()).toEqual({ asked: false })
})

it("offers no Ariadne review on a request that asks for my review when the repository already pins one", async () => {
  daemon(() => pull, [], [aRepository({ id: "repo", forge: aForge({ review_model: "stub:pin" }) })])
  renderScreen(<PullRequestPanel id="repo:42" onClose={() => {}} />)
  await screen.findByText("#42 Fix widgets")
  // The repository read lands a moment after the request's own: the button
  // this repository's pin refuses must not show even in that gap.
  await waitFor(() => expect(screen.queryByRole("button", { name: "Start review" })).toBeNull())
  expect(screen.getByText("someone")).toBeTruthy()
})

it("offers a manual Start review on a request that asks for my review when the repository pins none", async () => {
  daemon(() => pull, [], [aRepository({ id: "repo", forge: null })])
  renderScreen(<PullRequestPanel id="repo:42" onClose={() => {}} />)
  await screen.findByText("#42 Fix widgets")
  expect(screen.getByRole("button", { name: "Start review" })).toBeTruthy()
})

it("opens the console of a review resumed on the same session", async () => {
  const user = userEvent.setup()
  let row: PullRequestDto = {
    ...pull,
    role: "author",
    author_login: "me",
    review_model: "stub:review-model",
  }
  const ended = aSession({
    id: "01JSESS000000000000ENDED1",
    seat: "reviewer",
    status: "exited",
    goal_id: null,
    task_id: null,
    pull_request_id: "pull-42",
  })
  daemon(() => row)
  const answer = daemonFetch.getMockImplementation()
  daemonFetch.mockImplementation(async (input) => {
    const request = input as Request
    const path = new URL(request.url).pathname
    if (request.method === "PUT") row = { ...row, review_asked: true }
    // The daemon resumes the last review on its own row once asked.
    const review = row.review_asked ? { ...ended, status: "idle" as const } : ended
    if (path === "/v1/sessions") return jsonResponse(aSessionPage([review]))
    if (path === `/v1/sessions/${ended.id}`) return jsonResponse(review)
    return answer?.(input)
  })
  const { location } = renderScreen(<PullRequestPanel id="repo:42" onClose={() => {}} />, {
    route: "/forge/pull-requests?pr=repo%3A42",
  })

  await user.click(await screen.findByRole("button", { name: "Start review" }))
  const dialog = await screen.findByRole("dialog", { name: "Start an Ariadne review" })
  await user.click(within(dialog).getByRole("button", { name: "Start review" }))
  await waitFor(() => expect(location.url).toContain(`session=${ended.id}`), { timeout: 4000 })
})
