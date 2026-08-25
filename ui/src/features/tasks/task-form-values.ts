/**
 * The task form's shape: what it validates, what it starts from, and what it
 * sends.
 *
 * One schema for both modes. The repo is required only when the goal has more
 * than one (with a single repo the daemon infers it and the field is not even
 * shown), and the engineer only on create — edit mode neither shows nor sends
 * it. Everything else the daemon validates: profile roles, repo membership, dep
 * cycles, `max_tasks`, and for edits the pending/ready guard.
 */

import { z } from "zod"

import type { CreateTaskRequest, TaskDto, UpdateTaskRequest } from "@/api"

export function makeTaskFormSchema(opts: { creating: boolean; requireRepo: boolean }) {
  return z.object({
    title: z.string().trim().min(1, "Give the task a title."),
    description: z.string(),
    engineer_profile: opts.creating ? z.string().min(1, "Choose an engineer profile.") : z.string(),
    reviewers: z
      .array(z.object({ profile: z.string().min(1, "Choose a reviewer profile.") }))
      .min(1, "A task needs at least one reviewer.")
      .superRefine((rows, ctx) => {
        const seen = new Set<string>()
        rows.forEach((row, index) => {
          if (!row.profile) return
          if (seen.has(row.profile)) {
            ctx.addIssue({
              code: z.ZodIssueCode.custom,
              path: [index, "profile"],
              message: "This reviewer is already in the list.",
            })
          }
          seen.add(row.profile)
        })
      }),
    repo_id: opts.requireRepo ? z.string().min(1, "Choose a repository.") : z.string(),
    // Blank rows are dropped on submit, like the profile form's flags.
    depends_on: z.array(z.object({ task: z.string() })),
  })
}

export type TaskFormValues = z.infer<ReturnType<typeof makeTaskFormSchema>>

const CREATE_DEFAULTS: TaskFormValues = {
  title: "",
  description: "",
  engineer_profile: "",
  reviewers: [{ profile: "" }],
  repo_id: "",
  depends_on: [],
}

/** The task as it stands, or a blank form when there is none yet. */
export function taskToFormValues(task: TaskDto | undefined): TaskFormValues {
  if (!task) return CREATE_DEFAULTS
  return {
    title: task.title,
    description: task.description,
    engineer_profile: task.engineer_profile_id,
    reviewers: task.reviewers.map((reviewer) => ({ profile: reviewer.profile_id })),
    repo_id: "",
    depends_on: task.depends_on.map((dependency) => ({ task: dependency })),
  }
}

/** Blank dependency rows are dropped, and a task named twice counts once. */
function dependsOn(values: TaskFormValues): string[] {
  return [...new Set(values.depends_on.map((row) => row.task).filter(Boolean))]
}

/** `PATCH /v1/tasks/{id}` carries neither the engineer nor the repository. */
export function toUpdateTaskRequest(values: TaskFormValues): UpdateTaskRequest {
  return {
    title: values.title.trim(),
    description: values.description,
    reviewer_profiles: values.reviewers.map((row) => row.profile),
    depends_on: dependsOn(values),
  }
}

export function toCreateTaskRequest(
  values: TaskFormValues,
  /** Null where the goal has one repo and the daemon infers it. */
  repoId: string | null,
): CreateTaskRequest {
  return {
    title: values.title.trim(),
    description: values.description,
    engineer_profile: values.engineer_profile,
    reviewer_profiles: values.reviewers.map((row) => row.profile),
    repo_id: repoId,
    depends_on: dependsOn(values),
  }
}
