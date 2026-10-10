// @vitest-environment jsdom

import { screen, waitFor, within } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { useWorkflowParse } from "./use-workflow-parse"
import { WorkflowPreview } from "./workflow-preview"

function Preview({ document }: { document: string }) {
  return <WorkflowPreview parsed={useWorkflowParse(document)} />
}

function twoSteps() {
  const workflow = aWorkflow()
  const review = {
    ...workflow.steps[0],
    id: "review",
    title: "Review",
    skills: ["code-review"],
    rank: "frontier" as const,
    gate: null,
  }
  return { workflow, steps: [workflow.steps[0], review] }
}

describe("the workflow preview", () => {
  it("draws the steps as a numbered pipeline, in order, ending with a marker", async () => {
    const { workflow, steps } = twoSteps()
    daemonFetch.mockImplementation(async () => jsonResponse({ name: workflow.name, steps }))
    renderScreen(<Preview document={workflow.document} />)
    await screen.findByRole("heading", { name: "Develop" })

    const [first, second, end] = screen.getAllByRole("listitem")
    if (!first || !second || !end) throw new Error("expected three pipeline rows")
    expect(first.textContent).toContain("1")
    expect(within(first).getByRole("heading", { name: "Develop" })).toBeDefined()
    expect(second.textContent).toContain("2")
    expect(within(second).getByRole("heading", { name: "Review" })).toBeDefined()
    expect(end.textContent).toContain("End of task")
  })

  it("shows a card's title, id, description, skills and rank, in that order", async () => {
    const { workflow, steps } = twoSteps()
    daemonFetch.mockImplementation(async () => jsonResponse({ name: workflow.name, steps }))
    renderScreen(<Preview document={workflow.document} />)

    const heading = await screen.findByRole("heading", { name: "Develop" })
    const card = heading.closest("article")
    if (!card) throw new Error("expected the step to render as a card")
    expect(card.textContent).toMatch(
      /Develop[\s\S]*develop[\s\S]*Build the task\.[\s\S]*coding[\s\S]*balanced/,
    )
    expect(within(card).queryByText(/Gate:/)).toBeNull()
  })

  it("shows a step's gate as a chip on its connector, and none where a step has no gate", async () => {
    const { workflow, steps } = twoSteps()
    daemonFetch.mockImplementation(async () => jsonResponse({ name: workflow.name, steps }))
    renderScreen(<Preview document={workflow.document} />)

    await screen.findByRole("heading", { name: "Review" })
    expect(screen.getByText("Gate: committed")).toBeDefined()
    expect(screen.queryAllByText(/^Gate:/)).toHaveLength(1)
  })

  it("shows a parser refusal at its line, and dims the last good preview", async () => {
    const workflow = aWorkflow()
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ name: workflow.name, steps: workflow.steps }),
    )
    const { rerender } = renderScreen(<Preview document={workflow.document} />)
    await screen.findByRole("heading", { name: "Develop" })

    daemonFetch.mockImplementation(
      async () =>
        new Response(
          JSON.stringify({
            error: { code: "workflow_invalid", message: "Unknown rank", details: { line: 4 } },
          }),
          { status: 400, headers: { "content-type": "application/json" } },
        ),
    )
    rerender(<Preview document="broken" />)

    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("Line 4: Unknown rank"),
    )
    expect(screen.getByRole("heading", { name: "Develop" })).toBeDefined()
    expect(screen.getByRole("heading", { name: "Develop" }).closest("ol")?.className).toContain(
      "opacity-50",
    )
  })
})
