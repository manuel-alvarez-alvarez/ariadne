import { z } from "zod"

import type {
  AgentAssignment,
  CreateTaskRequest,
  TaskDto,
  UpdateTaskRequest,
  WorkflowStepDto,
} from "@/api"
import { modelRefField } from "@/features/models/model-ref"
import { parseSkillNames as parseSkills } from "@/features/skills/parse"

export function makeTaskFormSchema(opts: { requireRepo: boolean }) {
  return z.object({
    title: z.string().trim().min(1, "Give the task a title."),
    description: z.string(),
    agents: z.array(
      z.object({
        step: z.string(),
        skills: z.string(),
        model: modelRefField(),
        effort: z.string(),
      }),
    ),
    repo_id: opts.requireRepo ? z.string().min(1, "Choose a repository.") : z.string(),
    depends_on: z.array(z.object({ task: z.string() })),
  })
}

export type TaskFormValues = z.infer<ReturnType<typeof makeTaskFormSchema>>

export function taskToFormValues(
  task: TaskDto | undefined,
  steps: readonly WorkflowStepDto[],
): TaskFormValues {
  return {
    title: task?.title ?? "",
    description: task?.description ?? "",
    agents: steps.map((step) => {
      const agent = task?.agents.find((candidate) => candidate.step === step.id)
      return {
        step: step.id,
        skills: (agent?.skills ?? step.skills).join(", "),
        model: agent?.model ?? "",
        effort: agent?.effort ?? "",
      }
    }),
    repo_id: "",
    depends_on: (task?.depends_on ?? []).map((dependency) => ({ task: dependency })),
  }
}

function assignments(values: TaskFormValues): AgentAssignment[] {
  return values.agents.map((agent) => ({
    step: agent.step,
    skills: parseSkills(agent.skills),
    model: agent.model.trim(),
    ...(agent.effort.trim() ? { effort: agent.effort.trim() } : {}),
  }))
}

function dependsOn(values: TaskFormValues): string[] {
  return [...new Set(values.depends_on.map((row) => row.task).filter(Boolean))]
}

export function toCreateTaskRequest(values: TaskFormValues): CreateTaskRequest {
  return {
    title: values.title.trim(),
    description: values.description,
    ...(values.repo_id ? { repo_id: values.repo_id } : {}),
    agents: assignments(values),
    depends_on: dependsOn(values),
  }
}

export function toUpdateTaskRequest(values: TaskFormValues): UpdateTaskRequest {
  return {
    title: values.title.trim(),
    description: values.description,
    agents: assignments(values),
    depends_on: dependsOn(values),
  }
}
