// @vitest-environment jsdom

/**
 * Who a task thread's compose box lets the user address.
 *
 * The daemon refuses an addressee that takes no part in the thread
 * (`http/recipients.rs`), so the picker has to offer exactly its participants:
 * the engineer, the reviewer slots in their own order, and the goal's planner.
 * The user is not among them — they are the one writing — and neither is anyone
 * else's profile, however many the daemon knows about.
 *
 * Everything is seeded into the query cache: what the daemon returns is
 * `queries.ts`'s story, not this one's.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { type GoalDto, type ProfileDto, qk, type TaskDto } from "@/api"
import { aGoal, aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { TaskConversation } from "./task-conversation"

function profile(id: string, name: string, role: ProfileDto["role"]): ProfileDto {
  return {
    id,
    name,
    role,
    system_prompt: "",
    system_prompt_is_default: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  }
}

const PROFILES: ProfileDto[] = [
  profile("01PLANNER", "Planner", "planner"),
  profile("01ENGINEER", "Builder", "engineer"),
  profile("01REVIEWER", "Strict", "reviewer"),
  // Knows about this one too, and it works on nothing here.
  profile("01OTHER", "Bystander", "reviewer"),
]

const GOAL: GoalDto = aGoal({
  id: "01GOAL",
  title: "Ship it",
  description: "Ship it, all of it",
  planner_profile_id: "01PLANNER",
})

const TASK: TaskDto = aTask({
  id: "01TASK",
  repo_id: "01REPO",
  title: "Do the thing",
  branch: "do-the-thing-01task",
  engineer_profile_id: "01ENGINEER",
  reviewers: [{ profile_id: "01REVIEWER", agent_kind: null, model: null }],
  goal_id: GOAL.id,
})

function mount() {
  // Seeded data is never stale here: a background refetch would reach a daemon
  // that is not there, and its failure re-renders the thread out from under an
  // open picker.
  renderScreen(<TaskConversation taskId={TASK.id} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail(TASK.id), TASK)
      client.setQueryData(qk.tasks.messages(TASK.id), [])
      client.setQueryData(qk.goals.detail(GOAL.id), GOAL)
      client.setQueryData(qk.profiles.list({}), PROFILES)
    },
  })
}

it("offers the engineer, the reviewers and the planner, and no one else", async () => {
  mount()
  const user = userEvent.setup()

  await user.click(screen.getByRole("combobox", { name: "Addressee" }))
  const options = (await screen.findAllByRole("option")).map((option) => option.textContent)

  expect(options).toEqual(["the thread", "Builder", "Strict", "Planner"])
  expect(options).not.toContain("Bystander")
  expect(options).not.toContain("You")
})
