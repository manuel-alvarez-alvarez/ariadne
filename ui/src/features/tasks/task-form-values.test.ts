import { expect, it } from "vitest"

import type { WorkflowStepDto } from "@/api"
import { aTask } from "@/test/fixtures"
import { taskToFormValues, toCreateTaskRequest, toUpdateTaskRequest } from "./task-form-values"

const STEPS: WorkflowStepDto[] = [
  { id: "build", title: "Build", description: "", skills: ["coding"] },
  { id: "test", title: "Test", description: "", skills: ["testing"] },
]

it("maps one workflow agent for every column", () => {
  const values = taskToFormValues(aTask({ agents: [] }), STEPS)
  expect(values.agents).toEqual([
    { step: "build", skills: "coding", model: "", effort: "" },
    { step: "test", skills: "testing", model: "", effort: "" },
  ])
})

it("sends the complete workflow staffing on create and update", () => {
  const values = {
    ...taskToFormValues(undefined, STEPS),
    title: "Ship it",
    agents: [
      { step: "build", skills: "coding", model: "codex-acp:o3", effort: "high" },
      { step: "test", skills: "testing", model: "codex-acp:o3", effort: "" },
    ],
  }
  const agents = [
    { step: "build", skills: ["coding"], model: "codex-acp:o3", effort: "high" },
    { step: "test", skills: ["testing"], model: "codex-acp:o3" },
  ]
  expect(toCreateTaskRequest(values).agents).toEqual(agents)
  expect(toUpdateTaskRequest(values).agents).toEqual(agents)
})
