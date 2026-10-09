// @vitest-environment jsdom

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { aForge, aGoal, aModel, aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { IssuesPage } from "./issues-page"

it("lists forge issues and fills the goal dialog from a chosen issue", async () => {
  const user = userEvent.setup()
  const repository = aRepository({
    id: "repo-1",
    forge: {
      webhook: { state: "polling", url: null, error: null, last_delivery_at: null },
      kind: "github",
      host: "github.com",
      owner: "acme",
      name: "widgets",
      remote: "origin",
      enabled: true,
      login: "octocat",
      review_model: null,
      review_effort: null,
    },
  })
  const url = "https://github.com/acme/widgets/issues/12"
  let created: Record<string, unknown> | undefined
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const path = new URL(request.url).pathname
    if (path === "/v1/repositories") return jsonResponse([repository])
    if (path === `/v1/repositories/${repository.id}/issues`)
      return jsonResponse([
        {
          number: 12,
          title: "Fix the widget",
          body: "The widget fails.",
          url,
          labels: ["bug"],
          assignees: ["octocat"],
          updated_at: "2026-10-01T00:00:00Z",
        },
      ])
    if (path === "/v1/models") return jsonResponse([aModel()])
    if (path === "/v1/goals" && request.method === "POST") {
      created = await request.json()
      return jsonResponse(aGoal({ title: "Fix the widget", issue_url: url }), 201)
    }
    return new Response("not stubbed", { status: 404 })
  })
  renderScreen(<IssuesPage />, { route: "/forge/issues" })

  const link = await screen.findByRole("link", { name: "#12 Fix the widget" })
  expect(link.getAttribute("href")).toBe(url)
  expect(screen.getByText("bug")).toBeDefined()
  expect(screen.getByText("octocat")).toBeDefined()
  await user.click(screen.getByRole("button", { name: "Create goal" }))
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  expect(within(dialog).getByRole("textbox", { name: "Title" })).toHaveProperty(
    "value",
    "Fix the widget",
  )
  expect(
    within(dialog).getByText(/Issue: https:\/\/github.com\/acme\/widgets\/issues\/12/),
  ).toBeDefined()
  await user.click(within(dialog).getByRole("button", { name: "Orchestrator runs on" }))
  const models = await screen.findByRole("listbox", { name: "Models" })
  await user.click(within(models).getByText(aModel().id))
  await user.keyboard("{Escape}")
  await user.click(within(dialog).getByRole("button", { name: "Create goal" }))
  await waitFor(() => expect(created).toBeDefined())
  expect(created).toMatchObject({
    title: "Fix the widget",
    description: `The widget fails.\n\nIssue: ${url}`,
    repository_ids: [repository.id],
    issue_url: url,
  })
})

it("asks for issues assigned to me first, and for every open one once the switch is off", async () => {
  const user = userEvent.setup()
  const repository = aRepository({ id: "repo-1", forge: aForge({ enabled: true }) })
  const asked: string[] = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    if (url.pathname === "/v1/repositories") return jsonResponse([repository])
    asked.push(url.searchParams.get("assigned") ?? "")
    return jsonResponse([])
  })
  const { location } = renderScreen(<IssuesPage />, { route: "/forge/issues" })

  expect(await screen.findByText("No open issues are assigned to you")).toBeDefined()
  expect(asked).toContain("me")
  await user.click(screen.getByRole("switch", { name: "Assigned to me" }))
  expect(await screen.findByText("No open issues")).toBeDefined()
  expect(asked).toContain("all")
  expect(location.url).toBe("/forge/issues?assigned=all")

  // Refresh reads the issues from the forge again.
  const before = asked.length
  await user.click(screen.getByRole("button", { name: "Refresh" }))
  await waitFor(() => expect(asked.length).toBeGreaterThan(before))
})

it("narrows the issues to the words of their title or description", async () => {
  const user = userEvent.setup()
  const repository = aRepository({ id: "repo-1", forge: aForge({ enabled: true }) })
  const issue = (number: number, title: string, body: string) => ({
    number,
    title,
    body,
    url: `https://github.com/acme/widgets/issues/${number}`,
    labels: [],
    assignees: [],
    updated_at: "2026-10-01T00:00:00Z",
  })
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    if (new URL(request.url).pathname === "/v1/repositories") return jsonResponse([repository])
    return jsonResponse([
      issue(12, "Fix the widget", "The widget fails on an empty list."),
      issue(13, "Add gadgets", "Gadgets are missing."),
    ])
  })
  renderScreen(<IssuesPage />, { route: "/forge/issues" })
  await screen.findByRole("link", { name: "#13 Add gadgets" })

  await user.type(screen.getByRole("searchbox", { name: "Filter by text" }), "empty")
  await waitFor(() => expect(screen.queryByRole("link", { name: "#13 Add gadgets" })).toBeNull())
  expect(screen.getByRole("link", { name: "#12 Fix the widget" })).toBeDefined()
})
