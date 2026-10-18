// @vitest-environment jsdom

/**
 * Every goal and repository now runs a workflow, so the picker they share
 * has no "no workflow" choice to offer any more. A goal may still leave its
 * own pick to the repository it starts in — that is `allowInherit` — but a
 * repository has no repository behind it to leave that choice to.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useForm } from "react-hook-form"
import { describe, expect, it } from "vitest"

import { aWorkflow } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { WorkflowSelect } from "./workflow-select"

function Harness({ allowInherit }: { allowInherit?: boolean }) {
  const { control } = useForm<{ workflow: string }>({ defaultValues: { workflow: "" } })
  return (
    <WorkflowSelect
      control={control}
      name="workflow"
      id="workflow"
      enabled
      allowInherit={allowInherit}
    />
  )
}

describe("WorkflowSelect", () => {
  it("offers only named workflows by default, as a repository's own picker does", async () => {
    const user = userEvent.setup()
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const { pathname } = new URL(request.url)
      if (pathname === "/v1/workflows") return jsonResponse([aWorkflow({ name: "build" })])
      return new Response("not found", { status: 404 })
    })
    renderScreen(<Harness />)

    await user.click(screen.getByRole("combobox"))

    expect(await screen.findByRole("option", { name: "build" })).toBeDefined()
    expect(screen.queryByRole("option", { name: /repository default/i })).toBeNull()
    expect(screen.queryByRole("option", { name: /no workflow/i })).toBeNull()
  })

  it("offers the repository default beside the catalog where a goal's own pick allows it", async () => {
    const user = userEvent.setup()
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const { pathname } = new URL(request.url)
      if (pathname === "/v1/workflows") return jsonResponse([aWorkflow({ name: "build" })])
      return new Response("not found", { status: 404 })
    })
    renderScreen(<Harness allowInherit />)

    await user.click(screen.getByRole("combobox"))

    expect(await screen.findByRole("option", { name: "build" })).toBeDefined()
    expect(screen.getByRole("option", { name: "Repository default" })).toBeDefined()
    expect(screen.queryByRole("option", { name: /no workflow/i })).toBeNull()
  })
})
