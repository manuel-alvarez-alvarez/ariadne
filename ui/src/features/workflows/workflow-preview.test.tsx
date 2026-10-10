// @vitest-environment jsdom

import { screen, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { useWorkflowParse } from "./use-workflow-parse"
import { WorkflowPreview } from "./workflow-preview"

function Preview({ document }: { document: string }) {
  return <WorkflowPreview parsed={useWorkflowParse(document)} />
}

describe("the workflow preview", () => {
  it("renders each parsed column with its skill, rank and gate", async () => {
    const workflow = aWorkflow()
    const review = {
      ...workflow.steps[0],
      id: "review",
      title: "Review",
      skills: ["code-review"],
      rank: "frontier" as const,
      gate: null,
    }
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ name: workflow.name, steps: [workflow.steps[0], review] }),
    )
    renderScreen(<Preview document={workflow.document} />)

    expect(await screen.findByRole("heading", { name: "Develop" })).toBeDefined()
    expect(screen.getByText("coding")).toBeDefined()
    expect(screen.getByText("balanced")).toBeDefined()
    expect(screen.getByText("committed")).toBeDefined()
    expect(screen.getByRole("heading", { name: "Review" })).toBeDefined()
    expect(
      screen.getByRole("heading", { name: "Develop" }).closest("article")?.parentElement?.className,
    ).toContain("flex-col")
  })

  it("shows a parser refusal at its line", async () => {
    daemonFetch.mockImplementation(
      async () =>
        new Response(
          JSON.stringify({
            error: { code: "workflow_invalid", message: "Unknown rank", details: { line: 4 } },
          }),
          {
            status: 400,
            headers: { "content-type": "application/json" },
          },
        ),
    )
    renderScreen(<Preview document="broken" />)
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("Line 4: Unknown rank"),
    )
  })
})
