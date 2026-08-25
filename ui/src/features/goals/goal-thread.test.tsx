// @vitest-environment jsdom

/**
 * Who the planner thread's compose box lets the user address.
 *
 * Only the goal's planner works in this thread, and that is the whole rule the
 * daemon applies to it (`http/recipients.rs`): engineers and reviewers are
 * addressed in the task threads they work in, where which task is meant is not
 * in question. So the picker offers one name, however many profiles exist.
 *
 * Everything is seeded into the query cache: what the daemon returns is
 * `queries.ts`'s story, not this one's.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter } from "react-router-dom"
import { afterEach, expect, it } from "vitest"

import { type GoalDto, type ProfileDto, qk } from "@/api"

import { GoalThread } from "./goal-thread"

afterEach(cleanup)

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
]

const GOAL: GoalDto = {
  id: "01GOAL",
  title: "Ship it",
  description: "Ship it, all of it",
  status: "planning",
  planner_profile_id: "01PLANNER",
  required_approvals: 1,
  repos: [],
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

function mount() {
  // Seeded data is never stale here: a background refetch would reach a daemon
  // that is not there, and its failure re-renders the thread out from under an
  // open picker.
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
  })
  client.setQueryData(qk.goals.detail(GOAL.id), GOAL)
  client.setQueryData(qk.goals.messages(GOAL.id), [])
  client.setQueryData(qk.profiles.list({}), PROFILES)
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <GoalThread goalId={GOAL.id} />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

it("offers the goal's planner, and no one who works in a task thread", async () => {
  mount()
  const user = userEvent.setup()

  await user.click(screen.getByRole("combobox", { name: "Addressee" }))
  const options = (await screen.findAllByRole("option")).map((option) => option.textContent)

  expect(options).toEqual(["the thread", "Planner"])
})
