/**
 * What the task form seeds from, and what it sends.
 *
 * The pins are the whole of the subject here. An agent has no profile behind
 * it any more, so a pin is simply the pin — what used to need a rule about
 * which of two answers won is now a straight read, and what is left to pin
 * down is the update an edit sends: absent leaves the pin alone and a changed
 * value sends the concrete model.
 */

import { describe, expect, it } from "vitest"

import type { TaskAgentDto, TaskDto, WorkflowStepDto } from "@/api"
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
    model: over.model ?? "codex-acp:gpt-5.6",
  }
}

function task(agents: TaskAgentDto[], over: Partial<TaskDto> = {}): TaskDto {
  return aTask({ agents, ...over })
}

const BLANK: TaskFormValues = {
  title: "Wire it",
  description: "",
  author_skills: "coding",
  author_model: "codex-acp:gpt-5.6",
  author_effort: "",
  reviewers: [{ skills: "code-review", model: "codex-acp:gpt-5.6", effort: "" }],
  step_agents: [],
  repo_id: "",
  depends_on: [],
}

describe("seeding the form from a task", () => {
  it("reads each agent's skills and pin as they stand", () => {
    const values = taskToFormValues(
      task([
        agent("author", { skills: ["coding", "testing"], model: "codex-acp:o3", effort: "high" }),
        agent("reviewer", { skills: ["code-review"], model: "claude-agent-acp:claude-sonnet-5" }),
      ]),
    )
    expect(values.author_skills).toBe("coding, testing")
    expect(values.author_model).toBe("codex-acp:o3")
    expect(values.author_effort).toBe("high")
    expect(values.reviewers).toEqual([
      { skills: "code-review", model: "claude-agent-acp:claude-sonnet-5", effort: "" },
    ])
  })

  it("uses each agent's required model from the task", () => {
    const values = taskToFormValues(task([agent("author"), agent("reviewer")]))
    expect(values.author_model).toBe("codex-acp:gpt-5.6")
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
        author_model: "codex-acp:o3",
        author_effort: "high",
        reviewers: [
          { skills: "code-review", model: "claude-agent-acp:claude-sonnet-5", effort: "" },
          { skills: "security-review", model: "codex-acp:gpt-5.6", effort: "" },
        ],
      },
      null,
    )
    expect(body.agents).toEqual([
      { seat: "author", skills: ["coding", "testing"], model: "codex-acp:o3", effort: "high" },
      { seat: "reviewer", skills: ["code-review"], model: "claude-agent-acp:claude-sonnet-5" },
      { seat: "reviewer", skills: ["security-review"], model: "codex-acp:gpt-5.6" },
    ])
  })

  it("drops the blanks and the repeats out of a skills box", () => {
    const body = toCreateTaskRequest(
      { ...BLANK, author_skills: " coding , , testing ,coding" },
      null,
    )
    expect(body.agents[0]?.skills).toEqual(["coding", "testing"])
  })

  it("sends the concrete model where effort is empty", () => {
    const body = toCreateTaskRequest(BLANK, null)
    expect(body.agents[0]).toHaveProperty("model", "codex-acp:gpt-5.6")
    expect(body.agents[0]).not.toHaveProperty("effort")
  })
})

describe("updating a task", () => {
  const seeded = { model: "codex-acp:o3", effort: "high" }

  it("says nothing about a pin nobody touched", () => {
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: "codex-acp:o3", author_effort: "high" },
      seeded,
    )
    expect(body).not.toHaveProperty("model")
    expect(body).not.toHaveProperty("effort")
  })

  it("sends an empty effort to restore the model default", () => {
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: "codex-acp:o3", author_effort: "" },
      seeded,
    )
    expect(body).not.toHaveProperty("model")
    expect(body.effort).toBe("")
  })

  it("sends the model chosen, trimmed", () => {
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: " claude-agent-acp:claude-opus-5 ", author_effort: "" },
      seeded,
    )
    expect(body.model).toBe("claude-agent-acp:claude-opus-5")
  })

  it("moves the effort on its own, leaving the model where it is", () => {
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: "codex-acp:o3", author_effort: "xhigh" },
      seeded,
    )
    expect(body).not.toHaveProperty("model")
    expect(body.effort).toBe("xhigh")
  })

  it("sends the effort beside a moved model, though nobody touched it", () => {
    // The daemon drops the effort from a pin whose model moves, so the form's
    // reading of it has to travel with the model or it is silently lost.
    const body = toUpdateTaskRequest(
      { ...BLANK, author_model: "claude-agent-acp:claude-opus-5", author_effort: "high" },
      seeded,
    )
    expect(body.model).toBe("claude-agent-acp:claude-opus-5")
    expect(body.effort).toBe("high")
  })

  it("replaces the whole reviewer list, each with its skills and its pin", () => {
    const body = toUpdateTaskRequest(
      {
        ...BLANK,
        author_model: "codex-acp:o3",
        author_effort: "high",
        reviewers: [
          { skills: "code-review", model: "claude-agent-acp:claude-sonnet-5", effort: "" },
          { skills: "performance-review", model: "codex-acp:gpt-5.6", effort: "" },
        ],
      },
      seeded,
    )
    expect(body.reviewers).toEqual([
      { seat: "reviewer", skills: ["code-review"], model: "claude-agent-acp:claude-sonnet-5" },
      { seat: "reviewer", skills: ["performance-review"], model: "codex-acp:gpt-5.6" },
    ])
  })
})

describe("a task on a goal a workflow runs", () => {
  const STEPS: WorkflowStepDto[] = [
    { id: "develop", title: "Develop", description: "", skills: ["coding"] },
    { id: "review", title: "Review", description: "", skills: ["code-review"] },
  ]

  it("seeds one row per column, prefilled with the column's skills", () => {
    const values = taskToFormValues(undefined, STEPS)
    expect(values.step_agents).toEqual([
      { step: "develop", skills: "coding", model: "", effort: "" },
      { step: "review", skills: "code-review", model: "", effort: "" },
    ])
    expect(values.reviewers).toEqual([])
  })

  it("seeds each row from the agent staffed on its column", () => {
    const values = taskToFormValues(
      task([
        {
          id: "01AGENTREV",
          seat: "agent",
          step: "review",
          skills: ["security-review"],
          model: "codex-acp:o3",
          effort: "high",
        },
      ]),
      STEPS,
    )
    expect(values.step_agents).toEqual([
      { step: "develop", skills: "coding", model: "", effort: "" },
      { step: "review", skills: "security-review", model: "codex-acp:o3", effort: "high" },
    ])
  })

  const STAFFED: TaskFormValues = {
    ...BLANK,
    reviewers: [],
    step_agents: [
      { step: "develop", skills: "coding", model: "codex-acp:gpt-5.6", effort: "" },
      { step: "review", skills: "", model: "codex-acp:o3", effort: "high" },
    ],
  }

  it("creates one agent per column, seat agent, and no author or reviewer", () => {
    expect(toCreateTaskRequest(STAFFED, null).agents).toEqual([
      { seat: "agent", step: "develop", skills: ["coding"], model: "codex-acp:gpt-5.6" },
      { seat: "agent", step: "review", skills: [], model: "codex-acp:o3", effort: "high" },
    ])
  })

  it("replaces the whole list on edit, sending no author pin and no reviewers", () => {
    const body = toUpdateTaskRequest(STAFFED, { model: "", effort: "" })
    expect(body.agents).toHaveLength(2)
    expect(body).not.toHaveProperty("reviewers")
    expect(body).not.toHaveProperty("model")
  })
})
