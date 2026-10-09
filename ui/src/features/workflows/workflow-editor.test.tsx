// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { WorkflowEditor } from "./workflow-editor"

describe("the workflow editor", () => {
  it("keeps a dirty draft when a newer row arrives", async () => {
    const user = userEvent.setup()
    const workflow = aWorkflow()
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ name: workflow.name, steps: workflow.steps }),
    )
    const { rerender } = renderScreen(<WorkflowEditor workflow={workflow} onDeleted={vi.fn()} />)
    const document = screen.getByRole("textbox", { name: "Document" }) as HTMLTextAreaElement
    await user.type(document, "\nMine")
    rerender(
      <WorkflowEditor
        workflow={{ ...workflow, document: `${workflow.document}\nElsewhere` }}
        onDeleted={vi.fn()}
      />,
    )

    expect(document.value).toContain("Mine")
    expect(screen.getByText("This workflow changed elsewhere")).toBeDefined()
  })

  it("offers Reset for a shipped workflow and Delete for a user workflow", () => {
    const workflow = aWorkflow()
    daemonFetch.mockImplementation(async () =>
      jsonResponse({ name: workflow.name, steps: workflow.steps }),
    )
    const { rerender } = renderScreen(<WorkflowEditor workflow={workflow} onDeleted={vi.fn()} />)
    expect(screen.getByRole("button", { name: "Reset" })).toBeDefined()
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull()
    rerender(<WorkflowEditor workflow={{ ...workflow, builtin: false }} onDeleted={vi.fn()} />)
    expect(screen.getByRole("button", { name: "Delete" })).toBeDefined()
  })
})
