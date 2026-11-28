// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"
import { PinPicker } from "@/features/models/pin-picker"
import { LearnedPermissionsTab } from "@/features/permissions/learned-tab"
import { TunnelSettings } from "@/features/repositories/tunnel-settings"
import { WebhookPill } from "@/features/repositories/webhook-pill"
import { aForge, aTask } from "@/test/fixtures"
import { daemonFetch, renderScreen } from "@/test/harness"
import { CreateStandaloneTaskDialog } from "./create-standalone-task-dialog"
import { EditTaskDialog } from "./task-form-dialog"
import { TaskSummary } from "./task-summary"

function pending() {
  daemonFetch.mockImplementation(() => new Promise(() => {}))
}
function skeleton(label: string) {
  expect(screen.getByLabelText(label).getAttribute("data-slot")).toBe("skeleton")
}
it("shows a skeleton for the tunnel switch until its query answers", () => {
  pending()
  renderScreen(<TunnelSettings open />)
  skeleton("Loading tunnel")
  expect(screen.queryByRole("switch")).toBeNull()
})
it("shows skeletons for standalone repository and workflow fields", () => {
  pending()
  renderScreen(<CreateStandaloneTaskDialog open onOpenChange={() => {}} />)
  skeleton("Loading repositories")
  skeleton("Loading workflows")
})
it("shows skeletons for task fields until goal detail answers", () => {
  pending()
  renderScreen(
    <EditTaskDialog
      host="local"
      task={aTask({ execution_context: null })}
      open
      onOpenChange={() => {}}
    />,
  )
  skeleton("Loading repository")
  skeleton("Loading steps")
  skeleton("Loading dependencies")
})
it("shows a skeleton for unresolved dependency statuses", () => {
  pending()
  renderScreen(<TaskSummary task={aTask({ depends_on: ["dependency"] })} />)
  skeleton("Loading dependencies")
  expect(screen.queryByText(/Dependency status is not loaded/)).toBeNull()
})
it("shows a skeleton for the repository filter until repositories answer", () => {
  pending()
  renderScreen(<LearnedPermissionsTab />)
  skeleton("Loading repository filter")
})

it("shows a skeleton for webhook mode until the tunnel answers", () => {
  renderScreen(<WebhookPill forge={aForge({ enabled: true })} tunnel={undefined} />)
  skeleton("Loading webhook mode")
  expect(screen.queryByText(/smee|polling/)).toBeNull()
})
it("shows a skeleton in the pin picker until the catalog answers", async () => {
  pending()
  renderScreen(
    <PinPicker
      host="local"
      label="Model"
      models={undefined}
      model=""
      effort=""
      onChange={() => {}}
    />,
  )
  await userEvent.setup().click(screen.getByRole("button", { name: /Model/ }))
  skeleton("Loading catalog")
  expect(screen.queryByText("Loading the catalog…")).toBeNull()
})
