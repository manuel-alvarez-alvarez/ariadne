/**
 * What the task form starts from and what it sends, which is where the pins
 * carry their meaning.
 *
 * A slot's pin is one string — `<agent_kind>[:<model>]`, the agent CLI and the
 * model of it — with the effort that model is run at beside it, and an empty
 * box means the slot runs on its profile's own. That is a different request in
 * each mode: left out of a creation, and on an edit either left out (nothing
 * was said) or sent as the daemon's `"default"` sentinel (the box was
 * emptied). Which of the two an unpinned slot is depends on what the form
 * opened with, so the seeding and the update body are tested together.
 *
 * The one tie between the two fields is the daemon's: it drops the effort from
 * a pin whose model moves, so a model on the wire takes the effort box with it
 * whether that box changed or not.
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
  effort: "high",
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
  effort: "high",
  reviewers: [{ profile_id: REVIEWER.id, model: "claude_code", effort: null }],
})

/** The form as the dialog holds it, with only what a test is about set. */
function values(overrides: Partial<TaskFormValues> = {}): TaskFormValues {
  return {
    title: "Wire the strip",
    description: "",
    engineer_profile: ENGINEER.id,
    engineer_model: "",
    engineer_effort: "",
    reviewers: [{ profile: REVIEWER.id, model: "", effort: "" }],
    repo_id: "",
    depends_on: [],
    ...overrides,
  }
}

describe("seeding the form from a task", () => {
  it("reads a pin that is the profile's own model as no pin at all", () => {
    const seeded = taskToFormValues(TASK, PROFILES)

    expect(seeded.engineer_model).toBe("")
    expect(seeded.engineer_effort).toBe("")
    expect(seeded.reviewers).toEqual([{ profile: REVIEWER.id, model: "", effort: "" }])
  })

  it("shows the pinned model where it differs from the profile's", () => {
    const overridden: TaskDto = {
      ...TASK,
      model: "codex:gpt-5.3-codex",
      effort: "xhigh",
      reviewers: [{ profile_id: REVIEWER.id, model: "codex:gpt-5.3-codex", effort: "low" }],
    }

    const seeded = taskToFormValues(overridden, PROFILES)

    expect(seeded.engineer_model).toBe("codex:gpt-5.3-codex")
    expect(seeded.engineer_effort).toBe("xhigh")
    expect(seeded.reviewers).toEqual([
      { profile: REVIEWER.id, model: "codex:gpt-5.3-codex", effort: "low" },
    ])
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
    expect(blank.engineer_effort).toBe("")
    expect(blank.reviewers).toEqual([{ profile: "", model: "", effort: "" }])
  })
})

describe("creating a task", () => {
  it("carries the engineer's model, and each reviewer's", () => {
    const body = toCreateTaskRequest(
      values({
        engineer_model: "codex:gpt-5.3-codex",
        engineer_effort: "xhigh",
        reviewers: [
          { profile: REVIEWER.id, model: "claude_code:claude-fable-5", effort: "max" },
          { profile: "01JPROF00000000000000REV2", model: "", effort: "" },
        ],
      }),
      null,
    )

    expect(body.model).toBe("codex:gpt-5.3-codex")
    expect(body.effort).toBe("xhigh")
    // The unpinned one is left out entirely: that slot runs on its profile's.
    expect(body.reviewers).toEqual([
      { profile: REVIEWER.id, model: "claude_code:claude-fable-5", effort: "max" },
      { profile: "01JPROF00000000000000REV2" },
    ])
  })

  it("sends a bare agent CLI as typed, which is that CLI's own default", () => {
    const body = toCreateTaskRequest(
      values({
        engineer_model: "opencode",
        reviewers: [{ profile: REVIEWER.id, model: "codex", effort: "" }],
      }),
      null,
    )

    expect(body.model).toBe("opencode")
    expect(body.reviewers).toEqual([{ profile: REVIEWER.id, model: "codex" }])
  })

  it("leaves the model out when no box was filled in", () => {
    const body = toCreateTaskRequest(values(), null)

    expect("model" in body).toBe(false)
    expect("effort" in body).toBe(false)
    expect(body.reviewers).toEqual([{ profile: REVIEWER.id }])
  })

  it("carries an effort with no model beside it: the profile's model, at it", () => {
    const body = toCreateTaskRequest(
      values({
        engineer_effort: "max",
        reviewers: [{ profile: REVIEWER.id, model: "", effort: "low" }],
      }),
      null,
    )

    expect("model" in body).toBe(false)
    expect(body.effort).toBe("max")
    expect(body.reviewers).toEqual([{ profile: REVIEWER.id, effort: "low" }])
  })
})

describe("updating a task", () => {
  it("says nothing about a pin nobody touched", () => {
    const body = toUpdateTaskRequest(
      {
        ...values({ engineer_model: "codex:gpt-5.3-codex", engineer_effort: "high" }),
        title: "Renamed",
      },
      { model: "codex:gpt-5.3-codex", effort: "high" },
    )

    expect("model" in body).toBe(false)
    expect("effort" in body).toBe(false)
    expect(body.title).toBe("Renamed")
  })

  it("sends the daemon's sentinel for a box emptied back to the profile's own", () => {
    const body = toUpdateTaskRequest(values({ engineer_model: "" }), {
      model: "codex:gpt-5.3-codex",
      effort: "",
    })

    expect(body.model).toBe("default")
  })

  it("sends the model chosen, trimmed", () => {
    const body = toUpdateTaskRequest(values({ engineer_model: " codex:gpt-5.3-codex " }), {
      model: "",
      effort: "",
    })

    expect(body.model).toBe("codex:gpt-5.3-codex")
  })

  it("sends a bare agent CLI as itself, which is that CLI's own default", () => {
    const body = toUpdateTaskRequest(values({ engineer_model: "codex" }), {
      model: "codex:gpt-5.3-codex",
      effort: "",
    })

    expect(body.model).toBe("codex")
  })

  it("moves the effort on its own, leaving the model where it is", () => {
    const body = toUpdateTaskRequest(values({ engineer_effort: "max" }), {
      model: "",
      effort: "high",
    })

    expect("model" in body).toBe(false)
    expect(body.effort).toBe("max")
  })

  it("clears an emptied effort back to the agent CLI's own", () => {
    const body = toUpdateTaskRequest(values({ engineer_effort: "" }), {
      model: "",
      effort: "high",
    })

    expect(body.effort).toBe("default")
  })

  /**
   * The daemon drops the effort from a pin whose model moves, so the box has
   * to travel with it: the form said `xhigh` and the store must say `xhigh`.
   */
  it("sends the effort beside a moved model, though nobody touched it", () => {
    const body = toUpdateTaskRequest(
      values({ engineer_model: "codex:gpt-5.3-codex", engineer_effort: "xhigh" }),
      { model: "claude_code:claude-opus-5", effort: "xhigh" },
    )

    expect(body.model).toBe("codex:gpt-5.3-codex")
    expect(body.effort).toBe("xhigh")
  })

  it("replaces the whole reviewer list, each slot with its pin or none", () => {
    const body = toUpdateTaskRequest(
      values({
        reviewers: [
          { profile: REVIEWER.id, model: "codex:gpt-5.3-codex", effort: "" },
          { profile: "01JPROF00000000000000REV2", model: "", effort: "" },
        ],
      }),
      { model: "", effort: "" },
    )

    expect(body.reviewers).toEqual([
      { profile: REVIEWER.id, model: "codex:gpt-5.3-codex" },
      { profile: "01JPROF00000000000000REV2" },
    ])
  })
})
