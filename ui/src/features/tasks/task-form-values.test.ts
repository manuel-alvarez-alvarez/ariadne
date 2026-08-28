/**
 * What the task form starts from and what it sends, which is where the pins
 * carry their meaning.
 *
 * A slot's pin is one string — `<agent_kind>[:<model>]`, the agent CLI and the
 * model of it — and an empty box means the slot runs on its profile's own
 * model. That is a different request in each mode: left out of a creation, and
 * on an edit either left out (nothing was said) or sent as the daemon's
 * `"default"` sentinel (the box was emptied). Which of the two an unpinned slot
 * is depends on what the form opened with, so the seeding and the update body
 * are tested together.
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
  model: "claude_code:claude-opus-5",
})

const REVIEWER: ProfileDto = aProfile({
  id: "01JPROF000000000000000REV",
  name: "Reviewer",
  role: "reviewer",
  model: "claude_code",
})

const PROFILES = [ENGINEER, REVIEWER]

/** A task pinned to exactly what its profiles run on: no override anywhere. */
const TASK: TaskDto = aTask({
  status: "pending",
  engineer_profile_id: ENGINEER.id,
  model: "claude_code:claude-opus-5",
  reviewers: [{ profile_id: REVIEWER.id, model: "claude_code" }],
})

/** The form as the dialog holds it, with only what a test is about set. */
function values(overrides: Partial<TaskFormValues> = {}): TaskFormValues {
  return {
    title: "Wire the strip",
    description: "",
    engineer_profile: ENGINEER.id,
    engineer_model: "",
    reviewers: [{ profile: REVIEWER.id, model: "" }],
    repo_id: "",
    depends_on: [],
    ...overrides,
  }
}

describe("seeding the form from a task", () => {
  it("reads a pin that is the profile's own model as no pin at all", () => {
    const seeded = taskToFormValues(TASK, PROFILES)

    expect(seeded.engineer_model).toBe("")
    expect(seeded.reviewers).toEqual([{ profile: REVIEWER.id, model: "" }])
  })

  it("shows the pinned model where it differs from the profile's", () => {
    const overridden: TaskDto = {
      ...TASK,
      model: "codex:gpt-5.3-codex",
      reviewers: [{ profile_id: REVIEWER.id, model: "codex:gpt-5.3-codex" }],
    }

    const seeded = taskToFormValues(overridden, PROFILES)

    expect(seeded.engineer_model).toBe("codex:gpt-5.3-codex")
    expect(seeded.reviewers).toEqual([{ profile: REVIEWER.id, model: "codex:gpt-5.3-codex" }])
  })

  it("shows a bare agent CLI as itself, which is that CLI's own default model", () => {
    // The same CLI as the profile, but on that CLI's default rather than the
    // profile's model: a different string, so it is an override.
    const seeded = taskToFormValues({ ...TASK, model: "claude_code" }, PROFILES)

    expect(seeded.engineer_model).toBe("claude_code")
  })

  it("opens blank when there is no task to seed from", () => {
    const blank = taskToFormValues(undefined)

    expect(blank.engineer_model).toBe("")
    expect(blank.reviewers).toEqual([{ profile: "", model: "" }])
  })
})

describe("creating a task", () => {
  it("carries the engineer's model, and each reviewer's", () => {
    const body = toCreateTaskRequest(
      values({
        engineer_model: "codex:gpt-5.3-codex",
        reviewers: [
          { profile: REVIEWER.id, model: "claude_code:claude-fable-5" },
          { profile: "01JPROF00000000000000REV2", model: "" },
        ],
      }),
      null,
    )

    expect(body.model).toBe("codex:gpt-5.3-codex")
    // The unpinned one is left out entirely: that slot runs on its profile's.
    expect(body.reviewers).toEqual([
      { profile: REVIEWER.id, model: "claude_code:claude-fable-5" },
      { profile: "01JPROF00000000000000REV2" },
    ])
  })

  it("sends a bare agent CLI as typed, which is that CLI's own default", () => {
    const body = toCreateTaskRequest(
      values({
        engineer_model: "opencode",
        reviewers: [{ profile: REVIEWER.id, model: "codex" }],
      }),
      null,
    )

    expect(body.model).toBe("opencode")
    expect(body.reviewers).toEqual([{ profile: REVIEWER.id, model: "codex" }])
  })

  it("leaves the model out when no box was filled in", () => {
    const body = toCreateTaskRequest(values(), null)

    expect("model" in body).toBe(false)
    expect(body.reviewers).toEqual([{ profile: REVIEWER.id }])
  })
})

describe("updating a task", () => {
  it("says nothing about a pin nobody touched", () => {
    const body = toUpdateTaskRequest(
      { ...values({ engineer_model: "codex:gpt-5.3-codex" }), title: "Renamed" },
      "codex:gpt-5.3-codex",
    )

    expect("model" in body).toBe(false)
    expect(body.title).toBe("Renamed")
  })

  it("sends the daemon's sentinel for a box emptied back to the profile's own", () => {
    const body = toUpdateTaskRequest(values({ engineer_model: "" }), "codex:gpt-5.3-codex")

    expect(body.model).toBe("default")
  })

  it("sends the model chosen, trimmed", () => {
    const body = toUpdateTaskRequest(values({ engineer_model: " codex:gpt-5.3-codex " }), "")

    expect(body.model).toBe("codex:gpt-5.3-codex")
  })

  it("sends a bare agent CLI as itself, which is that CLI's own default", () => {
    const body = toUpdateTaskRequest(values({ engineer_model: "codex" }), "codex:gpt-5.3-codex")

    expect(body.model).toBe("codex")
  })

  it("replaces the whole reviewer list, each slot with its pin or none", () => {
    const body = toUpdateTaskRequest(
      values({
        reviewers: [
          { profile: REVIEWER.id, model: "codex:gpt-5.3-codex" },
          { profile: "01JPROF00000000000000REV2", model: "" },
        ],
      }),
      "",
    )

    expect(body.reviewers).toEqual([
      { profile: REVIEWER.id, model: "codex:gpt-5.3-codex" },
      { profile: "01JPROF00000000000000REV2" },
    ])
  })
})
