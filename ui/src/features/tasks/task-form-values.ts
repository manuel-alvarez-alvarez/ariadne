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
 * What each agent runs on is the one thing with a meaning of its own here: one
 * string, `<agent_kind>[:<model>]` — the agent CLI and, after a `:`, the model
 * of it (see `features/profiles/model-ref.ts`) — and, beside it, the effort
 * that model is run at. An empty box is the slot on its profile's own, and a
 * box holding exactly what the profile already says is the same thing said
 * twice — so a slot only carries a pin where it runs on something *else*,
 * which is what {@link taskToFormValues} seeds and what the two request
 * builders send:
 *
 * - on create, a slot with an empty box is sent with no `model` at all, and
 *   with no `effort` where that box is empty too;
 * - on edit, `UpdateTaskRequest.model` is a tri-state — absent leaves the pin
 *   alone, `"default"` puts it back on the profile's — so an untouched box
 *   sends nothing and one emptied sends the sentinel. Which of the two it is
 *   can only be answered against what the form was *seeded* with, hence the
 *   `initial` argument. `effort` is the same tri-state, with one tie between
 *   them: the daemon drops the effort from a pin whose model moves, so a model
 *   on the wire takes the effort box with it whether that box changed or not.
 *
 * Reviewers are sent whole either way, since the list replaces the task's, so
 * no baseline is needed there: each row goes out as the control on screen
 * reads, which is the reading the user saw. On a cold open that means a pin is
 * re-sent rather than dropped — the safe way round, since a slot sent without
 * a model is one put back on its profile's.
 */

import { z } from "zod"

import type { CreateTaskRequest, ProfileDto, TaskDto, UpdateTaskRequest } from "@/api"
import { modelRefField } from "@/features/profiles/model-ref"

/**
 * What the daemon reads as "not my choice": for a model, the slot back on its
 * profile's own; for the effort beside it, the agent CLI's own.
 */
const DEFAULT_SENTINEL = "default"

export function makeTaskFormSchema(opts: { creating: boolean; requireRepo: boolean }) {
  return z.object({
    title: z.string().trim().min(1, "Give the task a title."),
    description: z.string(),
    engineer_profile: opts.creating ? z.string().min(1, "Choose an engineer profile.") : z.string(),
    // Free text: the catalog is a suggestion, and a model it does not carry is
    // handed to the CLI named before the `:` as typed.
    engineer_model: modelRefField(),
    // The effort that model is run at. A closed list on screen, scoped by the
    // model beside it, so there is nothing here the daemon has not already
    // checked — and the empty string, which is that CLI's own effort.
    engineer_effort: z.string(),
    reviewers: z
      .array(
        z.object({
          profile: z.string().min(1, "Choose a reviewer profile."),
          model: modelRefField(),
          effort: z.string(),
        }),
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

/** The slot on its profile's own model, which is where a form starts. */
const NO_PIN = ""

const CREATE_DEFAULTS: TaskFormValues = {
  title: "",
  description: "",
  engineer_profile: "",
  engineer_model: NO_PIN,
  engineer_effort: NO_PIN,
  reviewers: [{ profile: "", model: NO_PIN, effort: NO_PIN }],
  repo_id: "",
  depends_on: [],
}

/**
 * One slot's box — its model, or the effort beside it: the pin, unless it is
 * only repeating what the profile already runs on.
 *
 * A pin that agrees with the profile shows as no pin at all — which is also
 * what leaves an untouched form saying nothing about the field. The two
 * fields are asked separately, since the daemon takes them separately: a slot
 * on its profile's own model at another effort is an effort pinned and no
 * model pinned. With the profiles not loaded yet there is nothing to compare
 * against — `own` is undefined — so the pin is shown as it stands.
 */
function overrideOf(pinned: string | null | undefined, own: string | null | undefined): string {
  if (!pinned) return NO_PIN
  return (own ?? null) === pinned ? NO_PIN : pinned
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
    engineer_model: overrideOf(task.model, profileOf(task.engineer_profile_id)?.model),
    engineer_effort: overrideOf(task.effort, profileOf(task.engineer_profile_id)?.effort),
    reviewers: task.reviewers.map((reviewer) => ({
      profile: reviewer.profile_id,
      model: overrideOf(reviewer.model, profileOf(reviewer.profile_id)?.model),
      effort: overrideOf(reviewer.effort, profileOf(reviewer.profile_id)?.effort),
    })),
    repo_id: "",
    depends_on: task.depends_on.map((dependency) => ({ task: dependency })),
  }
}

/** Blank dependency rows are dropped, and a task named twice counts once. */
function dependsOn(values: TaskFormValues): string[] {
  return [...new Set(values.depends_on.map((row) => row.task).filter(Boolean))]
}

/**
 * The pin of a slot that is assigned one, or nothing at all where it runs on
 * its profile's own model, at its profile's own effort.
 *
 * The two travel separately: an effort with no model beside it runs the
 * profile's own model at that effort, which is what the daemon does with it.
 */
function pinFields(model: string, effort: string): { model?: string; effort?: string } {
  const pin = model.trim()
  const at = effort.trim()
  return { ...(pin.length > 0 ? { model: pin } : {}), ...(at.length > 0 ? { effort: at } : {}) }
}

/** Each slot with what it runs on, or with nothing — which is its profile's. */
function reviewers(values: TaskFormValues): CreateTaskRequest["reviewers"] {
  return values.reviewers.map((row) => ({
    profile: row.profile,
    ...pinFields(row.model, row.effort),
  }))
}

/**
 * `PATCH /v1/tasks/{id}` carries neither the engineer nor the repository.
 *
 * `initial` is the engineer's model and effort as the form was last seeded —
 * what the user found in the boxes. It is what says whether a box reading "the
 * profile's own" was emptied, which asks for the sentinel, or was empty all
 * along, which asks for nothing. It has to be the *seeded* value rather than what
 * {@link taskToFormValues} would answer now: the seeding depends on profiles
 * that arrive after the dialog opens, and a form the user has already typed in
 * is deliberately not re-seeded, so the two part ways — and a control nobody
 * touched would go on the wire as a pin the user never chose.
 */
export function toUpdateTaskRequest(
  values: TaskFormValues,
  initial: { model: string; effort: string },
): UpdateTaskRequest {
  const pin = values.engineer_model.trim()
  const at = values.engineer_effort.trim()
  const body: UpdateTaskRequest = {
    title: values.title.trim(),
    description: values.description,
    reviewers: reviewers(values),
    depends_on: dependsOn(values),
  }
  const movedModel = pin !== initial.model.trim()
  const movedEffort = at !== initial.effort.trim()
  if (!movedModel && !movedEffort) return body
  return {
    ...body,
    // Emptied: the sentinel, which is the engineer profile's own model.
    // Otherwise the pin as it reads.
    ...(movedModel ? { model: pin.length > 0 ? pin : DEFAULT_SENTINEL } : {}),
    // An effort belongs to the model it is run at, and the daemon drops it
    // from a pin whose model moves — so a model on the wire takes the effort
    // box with it, and what is stored is what the form said.
    ...(movedModel || movedEffort ? { effort: at.length > 0 ? at : DEFAULT_SENTINEL } : {}),
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
    ...pinFields(values.engineer_model, values.engineer_effort),
    reviewers: reviewers(values),
    repo_id: repoId,
    depends_on: dependsOn(values),
  }
}
