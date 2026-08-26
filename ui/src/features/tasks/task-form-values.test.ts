/**
 * What the task form starts from and what it sends, which is where the models
 * carry their meaning.
 *
 * A model box means "run this on something else than the profile": empty is
 * the profile's own, and that is a different request in each mode — left out
 * of a creation, and on an edit either left out (nothing was said) or sent as
 * the daemon's `"default"` sentinel (the pin was taken off). Which of the two
 * an empty box is depends on what the form opened with, so the seeding and the
 * update body are tested together.
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
  model: "claude-opus-5",
})

const REVIEWER: ProfileDto = aProfile({
  id: "01JPROF000000000000000REV",
  name: "Reviewer",
  role: "reviewer",
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
    engineer_model: "",
    reviewers: [{ profile: REVIEWER.id, model: "" }],
    repo_id: "",
    depends_on: [],
    ...overrides,
  }
}

describe("seeding the form from a task", () => {
  it("leaves the model boxes empty where the pins are the profiles' own", () => {
    const seeded = taskToFormValues(TASK, PROFILES)

    expect(seeded.engineer_model).toBe("")
    expect(seeded.reviewers).toEqual([{ profile: REVIEWER.id, model: "" }])
  })

  it("fills each box with the pin that is not its profile's", () => {
    const overridden: TaskDto = {
      ...TASK,
      agent_kind: "codex",
      model: "gpt-5.3-codex",
      reviewers: [{ profile_id: REVIEWER.id, agent_kind: "codex", model: "gpt-5.3-codex" }],
    }

    const seeded = taskToFormValues(overridden, PROFILES)

    expect(seeded.engineer_model).toBe("gpt-5.3-codex")
    expect(seeded.reviewers).toEqual([{ profile: REVIEWER.id, model: "gpt-5.3-codex" }])
  })

  it("opens blank when there is no task to seed from", () => {
    const blank = taskToFormValues(undefined)

    expect(blank.engineer_model).toBe("")
    expect(blank.reviewers).toEqual([{ profile: "", model: "" }])
  })
})

describe("creating a task", () => {
  it("carries the engineer's model and each reviewer's", () => {
    const body = toCreateTaskRequest(
      values({
        engineer_model: "gpt-5.3-codex",
        reviewers: [
          { profile: REVIEWER.id, model: "claude-fable-5" },
          { profile: "01JPROF00000000000000REV2", model: "" },
        ],
      }),
      null,
    )

    expect(body.model).toBe("gpt-5.3-codex")
    // The blank one is left out entirely: that slot runs on its profile's own.
    expect(body.reviewers).toEqual([
      { profile: REVIEWER.id, model: "claude-fable-5" },
      { profile: "01JPROF00000000000000REV2" },
    ])
  })

  it("leaves the model out when nothing was chosen", () => {
    const body = toCreateTaskRequest(values(), null)

    expect("model" in body).toBe(false)
    expect(body.reviewers).toEqual([{ profile: REVIEWER.id }])
  })
})

describe("updating a task", () => {
  it("says nothing about a model box nobody touched", () => {
    const body = toUpdateTaskRequest(
      { ...values({ engineer_model: "gpt-5.3-codex" }), title: "Renamed" },
      "gpt-5.3-codex",
    )

    expect("model" in body).toBe(false)
    expect(body.title).toBe("Renamed")
  })

  it("sends the daemon's sentinel for a box that was emptied", () => {
    const body = toUpdateTaskRequest(values({ engineer_model: "" }), "gpt-5.3-codex")

    expect(body.model).toBe("default")
  })

  it("sends a model that was typed into an empty box", () => {
    const body = toUpdateTaskRequest(values({ engineer_model: " gpt-5.3-codex " }), "")

    expect(body.model).toBe("gpt-5.3-codex")
  })

  it("replaces the whole reviewer list, each slot with its model or none", () => {
    const body = toUpdateTaskRequest(
      values({
        reviewers: [
          { profile: REVIEWER.id, model: "gpt-5.3-codex" },
          { profile: "01JPROF00000000000000REV2", model: "" },
        ],
      }),
      "",
    )

    expect(body.reviewers).toEqual([
      { profile: REVIEWER.id, model: "gpt-5.3-codex" },
      { profile: "01JPROF00000000000000REV2" },
    ])
  })
})
