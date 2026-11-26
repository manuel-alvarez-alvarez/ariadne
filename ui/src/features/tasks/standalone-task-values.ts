import { z } from "zod"

import type { AgentAssignment, CreateStandaloneTaskRequest, WorkflowStepDto } from "@/api"
import { modelRefField } from "@/features/models/model-ref"
import { parseSkillNames as parseSkills } from "@/features/skills/parse"

/** The workflow field's value for a task that runs no workflow. */
export const NO_WORKFLOW = "-"

export const standaloneTaskSchema = z.object({
  title: z.string().trim().min(1, "Give the task a title."),
  description: z.string(),
  repo_id: z.string().min(1, "Choose a repository."),
  workflow: z.string(),
  host: z.string(),
  // One row per column of the workflow; one row with no `step` without one.
  agents: z.array(
    z.object({
      step: z.string(),
      skills: z.string(),
      model: modelRefField(),
      effort: z.string(),
    }),
  ),
})

export type StandaloneTaskValues = z.infer<typeof standaloneTaskSchema>

/** The staffing rows of a workflow's columns, or the one row of no workflow. */
export function staffingRows(
  steps: readonly WorkflowStepDto[] | null,
): StandaloneTaskValues["agents"] {
  if (steps === null) return [{ step: "", skills: "", model: "", effort: "" }]
  return steps.map((step) => ({
    step: step.id,
    skills: step.skills.join(", "),
    model: "",
    effort: "",
  }))
}

export function toCreateStandaloneTaskRequest(
  values: StandaloneTaskValues,
): CreateStandaloneTaskRequest {
  const workflow = values.workflow === NO_WORKFLOW ? null : values.workflow
  const agents: AgentAssignment[] = values.agents.map((agent) => ({
    ...(workflow ? { step: agent.step } : {}),
    skills: parseSkills(agent.skills),
    model: agent.model.trim(),
    ...(agent.effort.trim() ? { effort: agent.effort.trim() } : {}),
  }))
  return {
    title: values.title.trim(),
    description: values.description,
    repo_id: values.repo_id,
    ...(workflow ? { workflow } : {}),
    agents,
    ...(values.host && values.host !== "local" ? { host: values.host } : {}),
  }
}
