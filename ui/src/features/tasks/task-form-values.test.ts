/**
 * What the task form seeds from, and what it sends.
 *
 * The pins are the whole of the subject here. An agent has no profile behind
 * it any more, so a pin is simply the pin — what used to need a rule about
 * which of two answers won is now a straight read, and what is left to pin
 * down is the tri-state an edit sends: absent leaves the pin alone, the
 * sentinel puts it back on auto, anything else is the pin.
 */

import { describe, expect, it } from "vitest"

import type { TaskAgentDto, TaskDto } from "@/api"
import { aTask } from "@/test/fixtures"

import {
  type TaskFormValues,
  taskToFormValues,
  toCreateTaskRequest,
  toUpdateTaskRequest,
} from "./task-form-values"

function agent(seat: "author" | "reviewer", over: Partial<TaskAgentDto> = {}): TaskAgentDto {
  return {
    id: `01AGENT${seat}`,
    seat,
    skills: seat === "author" ? ["coding"] : ["code-review"],
    ...over,
  }
}

function task(agents: TaskAgentDto[], over: Partial<TaskDto> = {}): TaskDto {
  return aTask({ agents, ...over })
}

const BLANK: TaskFormValues = {
  title: "Wire it",
  description: "",
  author_skills: "coding",
  author_model: "",
  author_effort: "",
  reviewers: [{ skills: "code-review", model: "", effort: "" }],
  repo_id: "",
  depends_on: [],
}

describe("seeding the form from a task", () => {
  it("reads each agent's skills and pin as they stand", () => {
    const values = taskToFormValues(
      task([
        agent("author", { skills: ["coding", "testing"], model: "codex:o3", effort: "high" }),
        agent("reviewer", { skills: ["code-review"], model: "claude_code" }),
      ]),
    )
    expect(values.author_skills).toBe("coding, testing")
    expect(values.author_model).toBe("codex:o3")
    expect(values.author_effort).toBe("high")
    expect(values.reviewers).toEqual([{ skills: "code-review", model: "claude_code", effort: "" }])
  })

  it("shows an agent on auto as an empty box", () => {
    const values = taskToFormValues(task([agent("author"), agent("reviewer")]))
    expect(values.author_model).toBe("")
    expect(values.author_effort).toBe("")
  })

  it("opens blank when there is no task to seed from", () => {
    const values = taskToFormValues(undefined)
    expect(values.title).toBe("")
    expect(values.author_skills).toBe("coding")
    expect(values.reviewers).toHaveLength(1)
  })
})

describe("creating a task", () => {
  it("staffs the author first, then the reviewers in review order", () => {
    const body = toCreateTaskRequest(
      {
        ...BLANK,
        author_skills: "coding, testing",
        author_model: "codex:o3",
        author_effort: "high",
        reviewers: [
          { skills: "code-review", model: "claude_code", effort: "" },
          { skills: "security-review", model: "", effort: "" },
        ],
      },
      null,
    )
    expect(body.agents).toEqual([
      { seat: "author", skills: ["coding", "testing"], model: "codex:o3", effort: "high" },
      { seat: "reviewer", skills: ["code-review"], model: "claude_code" },
      { seat: "reviewer", skills: ["security-review"] },
    ])
  })

  it("drops the blanks and the repeats out of a skills box", () => {
    const body = toCreateTaskRequest(
      { ...BLANK, author_skills: " coding , , testing ,coding" },
      null,
    )
    expect(body.agents[0]?.skills).toEqual(["coding", "testing"])
  })

  it("leaves the model out when no box was filled in", () => {
    const body = toCreateTaskRequest(BLANK, null)
    expect(body.agents[0]).not.toHaveProperty("model")
    expect(body.agents[0]).not.toHaveProperty("effort")
  })
})

describe("updating a task", () => {
  const seeded = { model: "codex:o3", effort: "high" }

  it("says nothing about a pin nobody touched", () => {
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: "codex:o3", author_effort: "high" },
      seeded,
    )
    expect(body).not.toHaveProperty("model")
    expect(body).not.toHaveProperty("effort")
  })

  it("sends the sentinel for a box emptied back to auto", () => {
    const body = toUpdateTaskRequest({ ...BLANK, author_model: "", author_effort: "" }, seeded)
    expect(body.model).toBe("default")
    expect(body.effort).toBe("default")
  })

  it("sends the model chosen, trimmed", () => {
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: " claude_code:claude-opus-5 ", author_effort: "" },
      seeded,
    )
    expect(body.model).toBe("claude_code:claude-opus-5")
  })

  it("moves the effort on its own, leaving the model where it is", () => {
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: "codex:o3", author_effort: "xhigh" },
      seeded,
    )
    expect(body).not.toHaveProperty("model")
    expect(body.effort).toBe("xhigh")
  })

  it("sends the effort beside a moved model, though nobody touched it", () => {
    // The daemon drops the effort from a pin whose model moves, so the form's
    // reading of it has to travel with the model or it is silently lost.
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: "claude_code", author_effort: "high" },
      seeded,
    )
    expect(body.model).toBe("claude_code")
    expect(body.effort).toBe("high")
  })

  it("replaces the whole reviewer list, each with its skills and its pin", () => {
    const body = toUpdateTaskRequest(
      {
        ...BLANK,
        author_model: "codex:o3",
        author_effort: "high",
        reviewers: [
          { skills: "code-review", model: "claude_code", effort: "" },
          { skills: "performance-review", model: "", effort: "" },
        ],
      },
      seeded,
    )
    expect(body.reviewers).toEqual([
      { seat: "reviewer", skills: ["code-review"], model: "claude_code" },
      { seat: "reviewer", skills: ["performance-review"] },
    ])
  })
})
