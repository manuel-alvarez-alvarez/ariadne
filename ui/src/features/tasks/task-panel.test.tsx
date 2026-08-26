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
 * The pull request row is here because it exists only sometimes.
 *
 * The tokens are here for a different reason: the panel shows the daemon's own
 * aggregate twice over — the task's total in the facts, and the split by who
 * spent it above the sessions — and both have to be the figures the daemon
 * sent rather than anything added up here.
 *
 * Everything is seeded into the query cache: what the daemon returns is
 * `queries.ts`'s story, and the tabs are `task-panel.tsx`'s own.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { type ProfileDto, qk, type TaskDto } from "@/api"
import { renderScreen } from "@/test/harness"
import { TaskPanel } from "./task-panel"

const ENGINEER = "01JPROF0000000000000000ENG"
const STRICT = "01JPROF0000000000000STRICT"
const AUTO = "01JPROF00000000000000AUTO"

/** Every profile as it stands *today* — all three edited since the task. */
const PROFILES: ProfileDto[] = [
  {
    id: ENGINEER,
    name: "Builder",
    role: "engineer",
    agent_kind: "opencode",
    model: "grok-4",
    system_prompt: "",
    system_prompt_is_default: false,
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
    system_prompt_is_default: false,
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
    system_prompt_is_default: false,
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
  branch: "surface-the-pins-000001",
  depends_on: [],
  engineer_profile_id: ENGINEER,
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
  usage: {
    total: { input_tokens: 1_234_567, cached_input_tokens: 1_100_000, output_tokens: 45_300 },
    engineer: { input_tokens: 1_000_000, cached_input_tokens: 900_000, output_tokens: 40_000 },
    // Only the reviewer that has actually been spawned: the second slot has
    // never run, and the daemon lists no row for it.
    reviewers: [
      {
        profile_id: STRICT,
        profile_name: "Strict",
        usage: { input_tokens: 234_567, cached_input_tokens: 200_000, output_tokens: 5_300 },
      },
    ],
  },
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

function mount(task: TaskDto = TASK) {
  renderScreen(<TaskPanel taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail(task.id), task)
      client.setQueryData(qk.profiles.list({}), PROFILES)
    },
  })
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

/** The usage block above the sessions table, once that tab is open. */
async function breakdown(): Promise<HTMLElement> {
  await userEvent.setup().click(screen.getByRole("tab", { name: "Sessions" }))
  const heading = await screen.findByRole("heading", { name: "Tokens" })
  const block = heading.closest("section")
  if (!block) throw new Error("no usage block around the Tokens heading")
  return block
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

it("links the pull request its engineer published", () => {
  mount({
    ...TASK,
    status: "approved",
    pr_url: "https://github.com/owner/repo/pull/12",
  })

  const link = screen.getByRole("link", { name: "https://github.com/owner/repo/pull/12" })
  expect(link.getAttribute("href")).toBe("https://github.com/owner/repo/pull/12")
})

it("leaves the pull request row out of a task landed locally", () => {
  mount()

  expect(screen.queryByText("Pull request")).toBeNull()
})

it("shows the task's total, as the daemon aggregated it", () => {
  mount()

  expect(fact("Tokens")).toBe("in 1.2M (cached 1.1M) · out 45.3k")
})

it("says zero for a task whose agents have reported nothing", () => {
  mount({
    ...TASK,
    usage: {
      total: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
      engineer: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
      reviewers: [],
    },
  })

  expect(fact("Tokens")).toBe("in 0 (cached 0) · out 0")
})

it("breaks the total down by the agent that spent it, reviewers named", async () => {
  mount()
  const block = await breakdown()

  // The engineer reads as it does in the facts above — the task's pin, not
  // what its profile says today — and carries its own half of the total.
  expect(within(block).getByText("Engineer")).not.toBeNull()
  expect(block.textContent).toContain("Builder")
  expect(within(block).getByText("1.0M/40.0k")).not.toBeNull()

  // The reviewer that has run is named by its profile, with the pin of the
  // slot it fills; the one that never has is not a row at all.
  expect(within(block).getByText("Reviewer")).not.toBeNull()
  expect(block.textContent).toContain("Strict")
  expect(block.textContent).not.toContain("Second")
  expect(within(block).getByText("234.6k/5.3k")).not.toBeNull()

  // The total leads the block, and it is the task's own rather than the rows
  // under it added up.
  expect(block.textContent).toContain("in 1.2M (cached 1.1M) · out 45.3k")
})
