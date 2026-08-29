// @vitest-environment jsdom

/**
 * Who a task thread's compose box lets the user address, and what happens to
 * what they wrote in it.
 *
 * The daemon refuses an addressee that takes no part in the thread
 * (`http/recipients.rs`), so the picker has to offer exactly its participants:
 * the engineer, the reviewer slots in their own order, and the goal's planner.
 * The user is not among them — they are the one writing — and neither is anyone
 * else's profile, however many the daemon knows about.
 *
 * A task that is over has none of those participants left, so the box closes
 * instead of taking a message no session will be started to read. Up to that
 * point nothing typed into it is lost: the panel this thread lives in is
 * dismissed by any press outside it, and what comes back is what was being
 * written (see `thread-drafts.ts`).
 *
 * Everything is seeded into the query cache: what the daemon returns is
 * `queries.ts`'s story, not this one's.
 */

import { cleanup, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

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
  reviewers: [{ profile_id: "01REVIEWER", model: null }],
  goal_id: GOAL.id,
})

// A draft one test left behind is the next one's compose box already full.
beforeEach(() => {
  sessionStorage.clear()
})

function mount(task: TaskDto = TASK) {
  // Seeded data is never stale here: a background refetch would reach a daemon
  // that is not there, and its failure re-renders the thread out from under an
  // open picker.
  renderScreen(<TaskConversation taskId={task.id} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail(task.id), task)
      client.setQueryData(qk.tasks.messages(task.id), [])
      client.setQueryData(qk.goals.detail(GOAL.id), GOAL)
      client.setQueryData(qk.profiles.list({}), PROFILES)
    },
  })
}

/** The box, as the panel offers it. */
function composer(): HTMLTextAreaElement {
  return screen.getByRole("textbox", { name: "Message the task conversation" })
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

it("gives the half-written message back when the task is opened again", async () => {
  mount()
  const user = userEvent.setup()

  await user.type(composer(), "about that branch")
  // The panel is dismissed and the task opened again — a new thread, on the
  // same task.
  cleanup()
  mount()

  expect(composer().value).toBe("about that branch")
})

it("keeps one task's draft out of another's box", async () => {
  mount()
  const user = userEvent.setup()

  await user.type(composer(), "about that branch")
  cleanup()
  mount({ ...TASK, id: "01OTHERTASK" })

  expect(composer().value).toBe("")
})

it("closes the box on a task that is over", () => {
  mount({ ...TASK, status: "merged" })

  expect(composer().disabled).toBe(true)
  expect(screen.getByText("Merged: no agent is left to read this.")).toBeTruthy()
  expect(screen.queryByRole("combobox", { name: "Addressee" })).toBeNull()
})
