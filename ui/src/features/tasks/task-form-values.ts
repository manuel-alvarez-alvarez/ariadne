/**
 * The task form's shape: what it validates, what it starts from, and what it
 * sends.
 *
 * One schema for both modes. The repo is required only when the goal has more
 * than one (with a single repo the daemon infers it and the field is not even
 * shown), and the engineer only on create — edit mode neither shows nor sends
 * it. Everything else the daemon validates: profile roles, repo membership, dep
 * cycles, `max_tasks`, and for edits the pending/ready guard.
 *
 * The models are the one field with a meaning of their own here. The user picks
 * a model and nothing else — the daemon derives the agent CLI from it — and an
 * empty box means "whatever the profile runs on". So a box is only ever filled
 * where the task runs on something *else* than its profile, which is what
 * {@link taskToFormValues} seeds and what the two request builders send:
 *
 * - on create, a filled box is the model and an empty one is left out entirely;
 * - on edit, `UpdateTaskRequest.model` is a tri-state — absent leaves the pins
 *   alone, `"default"` puts them back on the profile's — so an untouched box
 *   sends nothing and an emptied one sends the sentinel. Which of the two it is
 *   can only be answered against the value the box was *seeded* with, hence the
 *   `initialModel` argument.
 *
 * Reviewers are sent whole either way, since the list replaces the task's, so
 * no baseline is needed there: each row goes out as the box on screen reads,
 * which is the reading the user saw. On a cold open that means a pin is
 * re-sent rather than dropped — the safe way round, since a slot sent without
 * a model is one put back on its profile's.
 */

import { z } from "zod"

import type { CreateTaskRequest, ProfileDto, TaskDto, UpdateTaskRequest } from "@/api"

/** What the daemon reads as "put the pins back on the profile's own". */
const DEFAULT_MODEL_SENTINEL = "default"

export function makeTaskFormSchema(opts: { creating: boolean; requireRepo: boolean }) {
  return z.object({
    title: z.string().trim().min(1, "Give the task a title."),
    description: z.string(),
    engineer_profile: opts.creating ? z.string().min(1, "Choose an engineer profile.") : z.string(),
    // Free text: the catalog is a suggestion and an id it does not carry is
    // still the daemon's to place or refuse (`unknown model …`, on the field).
    engineer_model: z.string(),
    reviewers: z
      .array(
        z.object({ profile: z.string().min(1, "Choose a reviewer profile."), model: z.string() }),
      )
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
  engineer_model: "",
  reviewers: [{ profile: "", model: "" }],
  repo_id: "",
  depends_on: [],
}

/**
 * The model box for one agent: the pin, unless it is only repeating what the
 * profile already runs on.
 *
 * The box means "run this on something else", so a pin that agrees with the
 * profile shows as empty — which is also what leaves an untouched form sending
 * nothing about the model at all. With the profiles not loaded yet there is
 * nothing to compare against, so the pin is shown as it stands.
 */
function overrideOf(pin: string | null | undefined, profile: ProfileDto | undefined): string {
  if (!pin) return ""
  return profile && profile.model === pin ? "" : pin
}

/**
 * The task as it stands, or a blank form when there is none yet.
 *
 * The profiles are what the pins are read against; without them every pin
 * reads as an override, which is wrong on screen but harmless on the wire.
 */
export function taskToFormValues(
  task: TaskDto | undefined,
  profiles: readonly ProfileDto[] | undefined = undefined,
): TaskFormValues {
  if (!task) return CREATE_DEFAULTS
  const profileOf = (id: string) => profiles?.find((profile) => profile.id === id)
  return {
    title: task.title,
    description: task.description,
    engineer_profile: task.engineer_profile_id,
    engineer_model: overrideOf(task.model, profileOf(task.engineer_profile_id)),
    reviewers: task.reviewers.map((reviewer) => ({
      profile: reviewer.profile_id,
      model: overrideOf(reviewer.model, profileOf(reviewer.profile_id)),
    })),
    repo_id: "",
    depends_on: task.depends_on.map((dependency) => ({ task: dependency })),
  }
}

/** Blank dependency rows are dropped, and a task named twice counts once. */
function dependsOn(values: TaskFormValues): string[] {
  return [...new Set(values.depends_on.map((row) => row.task).filter(Boolean))]
}

/** Each slot with the model chosen for it, or with none — which is its profile's. */
function reviewers(values: TaskFormValues): { profile: string; model?: string }[] {
  return values.reviewers.map((row) => {
    const model = row.model.trim()
    return model.length > 0 ? { profile: row.profile, model } : { profile: row.profile }
  })
}

/**
 * `PATCH /v1/tasks/{id}` carries neither the engineer nor the repository.
 *
 * `initialModel` is the engineer model box as the form was last seeded — what
 * the user found in it. It is what says whether an empty box was emptied,
 * which asks for the profile's own model back, or was empty all along, which
 * asks for nothing. It has to be the *seeded* value rather than what
 * {@link taskToFormValues} would answer now: the seeding depends on profiles
 * that arrive after the dialog opens, and a form the user has already typed in
 * is deliberately not re-seeded, so the two part ways — and a box nobody
 * touched would go on the wire as a pin the user never chose.
 */
export function toUpdateTaskRequest(
  values: TaskFormValues,
  initialModel: string,
): UpdateTaskRequest {
  const model = values.engineer_model.trim()
  const body: UpdateTaskRequest = {
    title: values.title.trim(),
    description: values.description,
    reviewers: reviewers(values),
    depends_on: dependsOn(values),
  }
  if (model === initialModel.trim()) return body
  return { ...body, model: model.length > 0 ? model : DEFAULT_MODEL_SENTINEL }
}

export function toCreateTaskRequest(
  values: TaskFormValues,
  /** Null where the goal has one repo and the daemon infers it. */
  repoId: string | null,
): CreateTaskRequest {
  const model = values.engineer_model.trim()
  return {
    title: values.title.trim(),
    description: values.description,
    engineer_profile: values.engineer_profile,
    // Left out rather than sent empty: the daemon refuses an empty model, and
    // an untouched box means the engineer profile's own.
    ...(model.length > 0 ? { model } : {}),
    reviewers: reviewers(values),
    repo_id: repoId,
    depends_on: dependsOn(values),
  }
}
