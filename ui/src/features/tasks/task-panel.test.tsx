// @vitest-environment jsdom

/**
 * What the panel says a task's agents run on.
 *
 * A profile is editable and a task is not: the agent CLI and the model land on
 * the task and on each reviewer slot when the task is created, and that is what
 * the daemon launches from. The panel used to render the profile's live values
 * instead, so the one moment the question matters — somebody moved the profile
 * onto another model — was the moment the panel lied about work already under
 * way.
 *
 * So both rows are read here against profiles that have since moved, including
 * the two reviewer slots that share nothing but their order, and the pin that
 * says `auto · default` rather than borrowing the profile's answer.
 *
 * The integrator is the exception, and is here to hold the difference: it is
 * assigned per task like the engineer but pinned to nothing, since it is only
 * picked up once the reviewers approve — so its row is what its profile says
 * today. The pull request row is here because it exists only sometimes.
 *
 * Everything is seeded into the query cache: what the daemon returns is
 * `queries.ts`'s story, and the tabs are `task-panel.tsx`'s own.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, expect, it } from "vitest"

import { type ProfileDto, qk, type TaskDto } from "@/api"
import { TooltipProvider } from "@/components/ui/tooltip"

import { TaskPanel } from "./task-panel"

afterEach(cleanup)

const ENGINEER = "01JPROF0000000000000000ENG"
const STRICT = "01JPROF0000000000000STRICT"
const AUTO = "01JPROF00000000000000AUTO"
const INTEGRATOR = "01JPROF0000000000000000INT"

/** Every profile as it stands *today* — all four edited since the task. */
const PROFILES: ProfileDto[] = [
  {
    id: ENGINEER,
    name: "Builder",
    role: "engineer",
    agent_kind: "opencode",
    model: "grok-4",
    system_prompt: "",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
  {
    id: STRICT,
    name: "Strict",
    role: "reviewer",
    agent_kind: "opencode",
    model: "grok-4",
    system_prompt: "",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
  {
    id: AUTO,
    name: "Second",
    role: "reviewer",
    agent_kind: "opencode",
    model: "grok-4",
    system_prompt: "",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
  {
    id: INTEGRATOR,
    name: "Lander",
    role: "integrator",
    agent_kind: "claude_code",
    model: "claude-opus-5",
    system_prompt: "",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
]

const TASK: TaskDto = {
  id: "01JTASK0000000000000000001",
  goal_id: "01JGOAL0000000000000000001",
  repo_id: "01JREPO0000000000000000001",
  title: "Surface the pins",
  description: "",
  status: "in_progress",
  branch: "ariadne/task-01JTASK0000000000000000001",
  depends_on: [],
  engineer_profile_id: ENGINEER,
  integrator_profile_id: INTEGRATOR,
  agent_kind: "codex",
  model: "gpt-5",
  reviewers: [
    { profile_id: STRICT, agent_kind: "claude_code", model: "claude-sonnet-5" },
    // Assigned on auto, with no model: the agent CLI is resolved at spawn time
    // and it takes that CLI's default. A pin like any other.
    { profile_id: AUTO, agent_kind: null, model: null },
  ],
  review_round: 0,
  stalled: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

function mount(task: TaskDto = TASK) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  client.setQueryData(qk.tasks.detail(task.id), task)
  client.setQueryData(qk.profiles.list({}), PROFILES)
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <TooltipProvider delay={0}>
          <TaskPanel taskId={task.id} onClose={() => {}} />
        </TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

/**
 * The value of a fact, by the label above it. The gap between a profile's name
 * and the two facts after it is a CSS one, so the text comes out run together;
 * one space is put back so the assertions read as the line does.
 */
function fact(label: string): string {
  const term = screen.getByText(label)
  const value = term.nextElementSibling
  if (!value) throw new Error(`no value under "${label}"`)
  return (value.textContent ?? "").replaceAll("·", " ·").replaceAll("  ·", " ·")
}

it("shows the engineer's pin, not what its profile says today", () => {
  mount()

  expect(fact("Engineer")).toContain("Builder")
  expect(fact("Engineer")).toContain("Codex · gpt-5")
  expect(fact("Engineer")).not.toContain("grok-4")
})

it("shows each reviewer slot's own pin, in review order", () => {
  mount()

  const reviewers = fact("Reviewers")
  expect(reviewers).toContain("Strict · Claude Code · claude-sonnet-5")
  // Both reviewer profiles now say `opencode · grok-4`; the slots do not.
  expect(reviewers).toContain("Second · auto · default")
  expect(reviewers).not.toContain("grok-4")
})

it("says a task has no reviewers rather than showing an empty list", () => {
  mount({ ...TASK, reviewers: [] })

  expect(fact("Reviewers")).toBe("none assigned")
})

it("shows the integrator as its profile stands, since nothing pinned it", () => {
  mount()

  // Assigned at creation like the engineer, run only once the reviewers are
  // done — so there is no snapshot to prefer over the profile.
  expect(fact("Integrator")).toBe("Lander · Claude Code · claude-opus-5")
})

it("links the pull request its integrator opened, by number", () => {
  mount({
    ...TASK,
    status: "integrating",
    pr_number: 12,
    pr_url: "https://github.com/owner/repo/pull/12",
  })

  const link = screen.getByRole("link", { name: "#12" })
  expect(link.getAttribute("href")).toBe("https://github.com/owner/repo/pull/12")
})

it("leaves the pull request row out of a task landed locally", () => {
  mount()

  expect(screen.queryByText("Pull request")).toBeNull()
})
