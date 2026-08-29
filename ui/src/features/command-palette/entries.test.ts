import { describe, expect, it } from "vitest"

import type { GoalDto, ProfileDto, SessionDto, TaskDto } from "@/api"
import { attentionTarget, collectAttention } from "@/features/goals/attention"
import { aGoal, aProfile, aSession, aTask } from "@/test/fixtures"
import { attentionEntries, buildPaletteEntries, paletteTargetTo } from "./entries"

const GOAL: GoalDto = aGoal({
  id: "01JGOAL00000000000000000A",
  title: "Ship the palette",
  planner_profile_id: "01JPROF0000000000000000AA",
})

const TASK: TaskDto = aTask({
  id: "01JTASK00000000000000000B",
  title: "Add the command palette",
  branch: "add-the-command-palette-01jtask",
  engineer_profile_id: "01JPROF0000000000000000AA",
  repo_id: "01JREPO0000000000000000AA",
  goal_id: GOAL.id,
})

const SESSION: SessionDto = aSession({
  id: "01JSESS00000000000000000C",
  goal_id: GOAL.id,
  task_id: TASK.id,
  profile_id: "01JPROF0000000000000000AA",
  tmux_session: "ariadne-eng-01jsess",
})

const PLANNER_SESSION: SessionDto = {
  ...SESSION,
  id: "01JSESS00000000000000000D",
  task_id: null,
  role: "planner",
  tmux_session: "ariadne-plan-01jsess",
}

const PROFILE: ProfileDto = aProfile({
  id: "01JPROF0000000000000000AA",
  name: "Reviewer (strict)",
  role: "reviewer",
  model: "codex:gpt-5",
  effort: "high",
})

const SOURCE = {
  goals: [GOAL],
  tasks: [TASK],
  sessions: [SESSION, PLANNER_SESSION],
  profiles: [PROFILE],
}

describe("buildPaletteEntries", () => {
  it("survives lists that have not loaded", () => {
    const entries = buildPaletteEntries({
      goals: undefined,
      tasks: undefined,
      sessions: undefined,
      profiles: undefined,
    })
    expect(entries).toEqual({ goals: [], tasks: [], sessions: [], profiles: [] })
  })

  it("makes a goal findable by its title and by its id", () => {
    const [entry] = buildPaletteEntries(SOURCE).goals
    expect(entry?.label).toBe("Ship the palette")
    expect(entry?.value).toContain("Ship the palette")
    // The id is searchable, but literally: see the note on `PaletteEntry`.
    expect(entry?.keywords).toContain(GOAL.id)
    expect(entry?.value).not.toContain(GOAL.id)
    expect(entry?.target).toEqual({ kind: "goal", goalId: GOAL.id })
  })

  it("makes a task findable by its branch, and shows it", () => {
    const [entry] = buildPaletteEntries(SOURCE).tasks
    expect(entry?.detail).toBe(TASK.branch)
    expect(entry?.value).toContain(TASK.branch)
    // Its goal's title too: tasks are looked for by the goal they belong to.
    expect(entry?.keywords).toContain(GOAL.title)
    expect(entry?.target).toEqual({ kind: "task", taskId: TASK.id })
  })

  it("names a session after its role and what it is working on", () => {
    const [engineer, planner] = buildPaletteEntries(SOURCE).sessions
    expect(engineer?.label).toBe("Engineer · Add the command palette")
    // A planner session has no task, so it is named after its goal.
    expect(planner?.label).toBe("Planner · Ship the palette")
    expect(engineer?.keywords).toContain(SESSION.id)
    expect(engineer?.target).toEqual({
      kind: "session",
      sessionId: SESSION.id,
      goalId: GOAL.id,
      taskId: TASK.id,
    })
    expect(planner?.target).toEqual({
      kind: "session",
      sessionId: PLANNER_SESSION.id,
      goalId: GOAL.id,
      taskId: null,
    })
  })

  it("falls back to the session's id when neither goal nor task is loaded", () => {
    const [entry] = buildPaletteEntries({ ...SOURCE, goals: [], tasks: [] }).sessions
    expect(entry?.label).toBe("Engineer")
    expect(entry?.detail).toBe("…0000000C")
  })

  it("lists profiles by name, with the role they fill", () => {
    const [entry] = buildPaletteEntries(SOURCE).profiles
    expect(entry?.label).toBe("Reviewer (strict)")
    expect(entry?.detail).toBe("Reviewer")
    // One keyword now carries both halves, since one string is the choice.
    expect(entry?.keywords).toContain("codex:gpt-5")
    // The effort is searchable too: it is half of what that profile runs as.
    expect(entry?.keywords).toContain("high")
    // The pick carries its subject: the screen expands that row.
    expect(entry?.target).toEqual({ kind: "page", path: `/profiles?expand=${PROFILE.id}` })
  })
})

/**
 * The rows are built from what the strip is showing, so they are asserted the
 * same way — through `collectAttention`, rather than against items written out
 * by hand that could stop being what the strip collects. What a row *is* is
 * `attention.ts`'s business (a task and the session stuck on it are one row);
 * what this pins is the shape a palette row takes around it, and that a pick
 * lands where the strip's own row would.
 */
describe("attentionEntries", () => {
  const FAILED = aTask({ ...TASK, id: "01JTASK0000000000000FAILED", status: "failed" })
  const WAITING: SessionDto = {
    ...SESSION,
    attention_reason: "waiting_user",
    attention_since: "2026-01-02T00:00:00Z",
  }

  it("names a stuck task by its title, with the reason it is on the list", () => {
    const [entry] = attentionEntries(collectAttention([GOAL], [FAILED], []))

    expect(entry?.label).toBe(FAILED.title)
    expect(entry?.detail).toBe("Failed")
    expect(entry?.keywords).toContain(FAILED.id)
  })

  it("leads with the session's reason on a row that carries one", () => {
    const [entry] = attentionEntries(collectAttention([GOAL], [TASK], [WAITING]))

    // The row is the task's — a task and the agent stuck on it are one thing
    // gone wrong — and what it is asking for is the session's reason.
    expect(entry?.label).toBe(TASK.title)
    expect(entry?.detail).toBe("Waiting for you")
    expect(entry?.keywords).toContain(WAITING.id)
  })

  it("names a planner's row by its role and goal, having no task", () => {
    const planner: SessionDto = { ...PLANNER_SESSION, attention_reason: "disconnected" }
    const [entry] = attentionEntries(collectAttention([GOAL], [], [planner]))

    expect(entry?.label).toBe(`Planner · ${GOAL.title}`)
    expect(entry?.detail).toBe("Disconnected")
  })

  it("tells two rows apart when they read the same", () => {
    // Two tasks of the same name, failed the same way: the rows are word for
    // word identical, and cmdk shows one row per *value*.
    const twin = aTask({ ...FAILED, id: "01JTASK00000000000000TWIN1" })
    const entries = attentionEntries(collectAttention([GOAL], [FAILED, twin], []))

    expect(entries).toHaveLength(2)
    expect(new Set(entries.map((entry) => entry.value)).size).toBe(2)
  })

  it("sends a pick exactly where the strip's own row goes", () => {
    const [item] = collectAttention([GOAL], [TASK], [WAITING])
    const [entry] = attentionEntries(collectAttention([GOAL], [TASK], [WAITING]))
    const search = new URLSearchParams("status=failed")
    if (!item || !entry) throw new Error("nothing was collected")

    // Same function, same answer: a question is answered in the thread it was
    // asked in, wherever the palette was opened.
    expect(paletteTargetTo(entry.target, search, "/goals")).toEqual(
      attentionTarget(item, search, "/goals"),
    )
  })

  it("has nothing to list when nothing is stuck", () => {
    expect(attentionEntries(collectAttention([GOAL], [TASK], [SESSION]))).toEqual([])
  })
})

describe("paletteTargetTo", () => {
  /** Any screen but the sessions one, where a task panel opens over the screen. */
  const OVER = "/goals"

  it("takes a goal to the board, where its panel lives", () => {
    expect(paletteTargetTo({ kind: "goal", goalId: "g1" }, new URLSearchParams(), OVER)).toBe(
      "/goals?goal=g1",
    )
  })

  it("stacks a task's panel on whatever the palette was opened over", () => {
    const target = paletteTargetTo(
      { kind: "task", taskId: "t1" },
      new URLSearchParams("goal=g1&status=active"),
      OVER,
    )
    expect(target).toEqual({ search: "?goal=g1&status=active&task=t1" })
  })

  it("drops the panel state the previous panel put on the URL", () => {
    const target = paletteTargetTo(
      { kind: "task", taskId: "t2" },
      new URLSearchParams("task=t1&tab=sessions&session=s1"),
      OVER,
    )
    expect(target).toEqual({ search: "?task=t2" })
  })

  it("opens a session inside its task's panel", () => {
    const target = paletteTargetTo(
      { kind: "session", sessionId: "s1", goalId: "g1", taskId: "t1" },
      new URLSearchParams(),
      OVER,
    )
    expect(target).toEqual({ search: "?task=t1&tab=sessions&session=s1" })
  })

  it("takes a planner session to the goal panel, the only place it shows", () => {
    const target = paletteTargetTo(
      { kind: "session", sessionId: "s2", goalId: "g1", taskId: null },
      new URLSearchParams("task=t1"),
      OVER,
    )
    expect(target).toBe("/goals?goal=g1&tab=sessions&session=s2")
  })

  it("opens a task on the board from the screen whose `?task=` is a filter", () => {
    // On the sessions screen that param narrows the list; a pick has to open
    // what its row names, so it lands on the board instead.
    expect(
      paletteTargetTo({ kind: "task", taskId: "t1" }, new URLSearchParams("goal=g1"), "/sessions"),
    ).toEqual({ pathname: "/goals", search: "?task=t1" })
  })

  it("passes a page straight through", () => {
    expect(paletteTargetTo({ kind: "page", path: "/sessions" }, new URLSearchParams(), OVER)).toBe(
      "/sessions",
    )
  })
})
