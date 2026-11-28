// @vitest-environment jsdom
import { screen } from "@testing-library/react"
import { expect, it } from "vitest"

import { qk } from "@/api"
import { aGoal, GOAL_ID } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { GoalPanel } from "./goal-panel"

it("shows the goal's state in the pane as a pill with its word", async () => {
  const goal = aGoal({ id: GOAL_ID, status: "active" })
  renderScreen(<GoalPanel host="local" goalId={goal.id} onClose={() => {}} />, {
    seed: (client) => client.setQueryData(qk.goals.detail("local", goal.id), goal),
  })

  const pill = await screen.findByText("Active")
  expect(pill.className).toContain("rounded-full")
  expect(screen.queryByRole("img", { name: "Active" })).toBeNull()
})
