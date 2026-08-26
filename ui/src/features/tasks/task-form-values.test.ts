/**
 * What the task form starts from and what it sends, which is where the pins
 * carry their meaning.
 *
 * A slot's pin is a pair — an agent CLI, and a model narrowing it — and no
 * agent means the slot runs on its profile's own agent and model both. That is
 * a different request in each mode: left out of a creation, and on an edit
 * either left out (nothing was said) or sent as the daemon's `"default"`
 * sentinel (the pin was taken off). Which of the two an unpinned slot is
 * depends on what the form opened with, so the seeding and the update body are
 * tested together.
 */

import { describe, expect, it } from "vitest"

import type { ProfileDto, TaskDto } from "@/api"
import { aProfile, aTask } from "@/test/fixtures"

import {
  type TaskFormValues,
  taskToFormValues,
  toCreateTaskRequest,
  toUpdateTaskRequest,
} from "./task-form-values"

const ENGINEER: ProfileDto = aProfile({
  id: "01JPROF000000000000000ENG",
  name: "Engineer",
  agent_kind: "claude_code",
  model: "claude-opus-5",
})

const REVIEWER: ProfileDto = aProfile({
  id: "01JPROF000000000000000REV",
  name: "Reviewer",
  role: "reviewer",
  agent_kind: "claude_code",
  model: null,
})

const PROFILES = [ENGINEER, REVIEWER]

/** A task pinned to exactly what its profiles run on: no override anywhere. */
const TASK: TaskDto = aTask({
  status: "pending",
  engineer_profile_id: ENGINEER.id,
  agent_kind: "claude_code",
  model: "claude-opus-5",
  reviewers: [{ profile_id: REVIEWER.id, agent_kind: "claude_code", model: null }],
})

/** The form as the dialog holds it, with only what a test is about set. */
function values(overrides: Partial<TaskFormValues> = {}): TaskFormValues {
  return {
    title: "Wire the strip",
    description: "",
    engineer_profile: ENGINEER.id,
    engineer_agent: "default",
    engineer_model: "",
    reviewers: [{ profile: REVIEWER.id, agent: "default", model: "" }],
    repo_id: "",
    depends_on: [],
    ...overrides,
  }
}

describe("seeding the form from a task", () => {
  it("reads a pin that is the profile's own pair as no pin at all", () => {
    const seeded = taskToFormValues(TASK, PROFILES)

    expect(seeded.engineer_agent).toBe("default")
    expect(seeded.engineer_model).toBe("")
    expect(seeded.reviewers).toEqual([{ profile: REVIEWER.id, agent: "default", model: "" }])
  })

  it("shows the pinned agent, and the model beside it, where the pair differs", () => {
    const overridden: TaskDto = {
      ...TASK,
      agent_kind: "codex",
      model: "gpt-5.3-codex",
      reviewers: [{ profile_id: REVIEWER.id, agent_kind: "codex", model: "gpt-5.3-codex" }],
    }

    const seeded = taskToFormValues(overridden, PROFILES)

    expect(seeded.engineer_agent).toBe("codex")
    expect(seeded.engineer_model).toBe("gpt-5.3-codex")
    expect(seeded.reviewers).toEqual([
      { profile: REVIEWER.id, agent: "codex", model: "gpt-5.3-codex" },
    ])
  })

  it("shows the agent with an empty box where the pin names no model", () => {
    // Same CLI as the profile, but on that CLI's default rather than the
    // profile's model: the pair differs, so it is an override.
    const seeded = taskToFormValues({ ...TASK, agent_kind: "claude_code", model: null }, PROFILES)

    expect(seeded.engineer_agent).toBe("claude_code")
    expect(seeded.engineer_model).toBe("")
  })

  it("opens blank when there is no task to seed from", () => {
    const blank = taskToFormValues(undefined)

    expect(blank.engineer_agent).toBe("default")
    expect(blank.engineer_model).toBe("")
    expect(blank.reviewers).toEqual([{ profile: "", agent: "default", model: "" }])
  })
})

describe("creating a task", () => {
  it("carries the engineer's agent and model, and each reviewer's", () => {
    const body = toCreateTaskRequest(
      values({
        engineer_agent: "codex",
        engineer_model: "gpt-5.3-codex",
        reviewers: [
          { profile: REVIEWER.id, agent: "claude_code", model: "claude-fable-5" },
          { profile: "01JPROF00000000000000REV2", agent: "default", model: "" },
        ],
      }),
      null,
    )

    expect(body.agent_kind).toBe("codex")
    expect(body.model).toBe("gpt-5.3-codex")
    // The unpinned one is left out entirely: that slot runs on its profile's.
    expect(body.reviewers).toEqual([
      { profile: REVIEWER.id, agent_kind: "claude_code", model: "claude-fable-5" },
      { profile: "01JPROF00000000000000REV2" },
    ])
  })

  it("sends the agent alone where its box is empty, which is that CLI's default", () => {
    const body = toCreateTaskRequest(
      values({
        engineer_agent: "opencode",
        reviewers: [{ profile: REVIEWER.id, agent: "codex", model: "" }],
      }),
      null,
    )

    expect(body.agent_kind).toBe("opencode")
    expect("model" in body).toBe(false)
    expect(body.reviewers).toEqual([{ profile: REVIEWER.id, agent_kind: "codex" }])
  })

  it("leaves both out when no agent was chosen", () => {
    const body = toCreateTaskRequest(values(), null)

    expect("agent_kind" in body).toBe(false)
    expect("model" in body).toBe(false)
    expect(body.reviewers).toEqual([{ profile: REVIEWER.id }])
  })
})

describe("updating a task", () => {
  it("says nothing about a pin nobody touched", () => {
    const body = toUpdateTaskRequest(
      {
        ...values({ engineer_agent: "codex", engineer_model: "gpt-5.3-codex" }),
        title: "Renamed",
      },
      { agent: "codex", model: "gpt-5.3-codex" },
    )

    expect("agent_kind" in body).toBe(false)
    expect("model" in body).toBe(false)
    expect(body.title).toBe("Renamed")
  })

  it("sends the daemon's sentinel for a slot put back on its profile's own", () => {
    const body = toUpdateTaskRequest(values({ engineer_agent: "default", engineer_model: "" }), {
      agent: "codex",
      model: "gpt-5.3-codex",
    })

    expect(body.agent_kind).toBe("default")
    expect("model" in body).toBe(false)
  })

  it("sends the agent and the model chosen beside it", () => {
    const body = toUpdateTaskRequest(
      values({ engineer_agent: "codex", engineer_model: " gpt-5.3-codex " }),
      { agent: "default", model: "" },
    )

    expect(body.agent_kind).toBe("codex")
    expect(body.model).toBe("gpt-5.3-codex")
  })

  it("sends the agent alone when its box was emptied, which is that CLI's default", () => {
    const body = toUpdateTaskRequest(values({ engineer_agent: "codex", engineer_model: "" }), {
      agent: "codex",
      model: "gpt-5.3-codex",
    })

    expect(body.agent_kind).toBe("codex")
    expect("model" in body).toBe(false)
  })

  it("replaces the whole reviewer list, each slot with its pin or none", () => {
    const body = toUpdateTaskRequest(
      values({
        reviewers: [
          { profile: REVIEWER.id, agent: "codex", model: "gpt-5.3-codex" },
          { profile: "01JPROF00000000000000REV2", agent: "default", model: "" },
        ],
      }),
      { agent: "default", model: "" },
    )

    expect(body.reviewers).toEqual([
      { profile: REVIEWER.id, agent_kind: "codex", model: "gpt-5.3-codex" },
      { profile: "01JPROF00000000000000REV2" },
    ])
  })
})
