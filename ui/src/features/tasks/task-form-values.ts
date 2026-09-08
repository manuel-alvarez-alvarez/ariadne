/**
 * The task form's shape: what it validates, what it starts from, and what it
 * sends.
 *
 * One schema for both modes. The repo is required only when the goal has more
 * than one (with a single repo the daemon infers it and the field is not even
 * shown), and the author is staffed only on create — a task keeps the author
 * it started with, so edit mode neither shows nor sends it. Everything else
 * the daemon validates: the skill names, repo membership, dep cycles,
 * and for edits the pending/ready guard.
 *
 * Two things carry a meaning of their own here.
 *
 * **Skills** are what an agent is: a comma-separated list of names, in the
 * order they reach it. They are free text against a catalog — the daemon
 * refuses a name no skill answers to, and the picker offers the ones it has —
 * because the catalog is the user's to extend.
 *
 * **What an agent runs on** is one string, `<agent_kind>:<model>` — the CLI
 * and, after a `:`, the model of it (see `features/models/model-ref.ts`) — and
 * beside it the effort that model is run at. A model is required, and an empty
 * effort uses the model's default effort.
 *
 * On edit, an absent `UpdateTaskRequest.model` leaves the pin alone. Which
 * fields moved is answered against what the form was *seeded* with, hence the
 * `initial` argument. `effort` has one tie to the model: the
 * daemon drops the effort from a pin whose model moves, so a model on the wire
 * takes the effort box with it whether that box changed or not.
 *
 * Reviewers are sent whole either way, since the list replaces the task's, so
 * no baseline is needed there.
 */

import { z } from "zod"

import type { AgentAssignment, CreateTaskRequest, TaskDto, UpdateTaskRequest } from "@/api"
import { modelRefField } from "@/features/models/model-ref"

import { parseSkillNames as parseSkills } from "@/features/skills/parse"

import { taskAuthor, taskReviewers } from "./agents"

/** The skills of one agent, as the form holds them and as it reads them back. */
function formatSkills(skills: readonly string[]): string {
  return skills.join(", ")
}

/** A field holding one agent's skills: at least one, once the blanks are out. */
function skillsField(message: string) {
  return z.string().refine((typed) => parseSkills(typed).length > 0, { message })
}

export function makeTaskFormSchema(opts: { creating: boolean; requireRepo: boolean }) {
  return z.object({
    title: z.string().trim().min(1, "Give the task a title."),
    description: z.string(),
    author_skills: opts.creating ? skillsField("Give the author at least one skill.") : z.string(),
    // Free text: the catalog is a suggestion, and a model it does not carry is
    // handed to the CLI named before the `:` as typed.
    author_model: modelRefField(),
    // The effort that model is run at. A closed list on screen, scoped by the
    // model beside it, so there is nothing here the daemon has not already
    // checked — and the empty string, which is that CLI's own effort.
    author_effort: z.string(),
    // No minimum: most work is worth a second pair of eyes, and a task
    // starts with one reviewer for that reason, but some has nothing to
    // review — a release, a dependency bump the suite already judged — and
    // such a task is approved as soon as its author asks.
    reviewers: z.array(
      z.object({
        skills: skillsField("Give the reviewer at least one skill."),
        model: modelRefField(),
        effort: z.string(),
      }),
    ),
    // How the task ends, agreed with the user when it is written: a closed
    // list, since the daemon takes exactly these three.
    landing: z.enum(["merge", "pull_request", "none"]),
    repo_id: opts.requireRepo ? z.string().min(1, "Choose a repository.") : z.string(),
    // Blank rows are dropped on submit.
    depends_on: z.array(z.object({ task: z.string() })),
  })
}

export type TaskFormValues = z.infer<ReturnType<typeof makeTaskFormSchema>>

const CREATE_DEFAULTS: TaskFormValues = {
  title: "",
  description: "",
  author_skills: "coding",
  author_model: "",
  author_effort: "",
  reviewers: [{ skills: "code-review", model: "", effort: "" }],
  // The way most repositories take a change, and the way the daemon defaults
  // a task nobody said otherwise about.
  landing: "merge",
  repo_id: "",
  depends_on: [],
}

/** The task as it stands, or a blank form when there is none yet. */
export function taskToFormValues(task: TaskDto | undefined): TaskFormValues {
  if (!task) return CREATE_DEFAULTS
  const author = taskAuthor(task)
  return {
    title: task.title,
    description: task.description,
    author_skills: formatSkills(author?.skills ?? []),
    author_model: author?.model ?? "",
    author_effort: author?.effort ?? "",
    reviewers: taskReviewers(task).map((reviewer) => ({
      skills: formatSkills(reviewer.skills),
      model: reviewer.model,
      effort: reviewer.effort ?? "",
    })),
    landing: task.landing,
    repo_id: "",
    depends_on: task.depends_on.map((dependency) => ({ task: dependency })),
  }
}

/** Blank dependency rows are dropped, and a task named twice counts once. */
function dependsOn(values: TaskFormValues): string[] {
  return [...new Set(values.depends_on.map((row) => row.task).filter(Boolean))]
}

/**
 * The pin of an agent. The model is always present and effort stays optional.
 */
function pinFields(model: string, effort: string): { model: string; effort?: string } {
  const pin = model.trim()
  const at = effort.trim()
  return { model: pin, ...(at.length > 0 ? { effort: at } : {}) }
}

/** The reviewers as the daemon staffs them, in review order. */
function reviewers(values: TaskFormValues): AgentAssignment[] {
  return values.reviewers.map((row) => ({
    seat: "reviewer" as const,
    skills: parseSkills(row.skills),
    ...pinFields(row.model, row.effort),
  }))
}

/**
 * `PATCH /v1/tasks/{id}` carries neither the author's staffing nor the
 * repository.
 *
 * `initial` is the author's model and effort as the form was last seeded —
 * what the user found in the boxes.
 */
export function toUpdateTaskRequest(
  values: TaskFormValues,
  initial: { model: string; effort: string },
): UpdateTaskRequest {
  const pin = values.author_model.trim()
  const at = values.author_effort.trim()
  const body: UpdateTaskRequest = {
    title: values.title.trim(),
    description: values.description,
    reviewers: reviewers(values),
    depends_on: dependsOn(values),
    landing: values.landing,
  }
  const movedModel = pin !== initial.model.trim()
  const movedEffort = at !== initial.effort.trim()
  if (!movedModel && !movedEffort) return body
  return {
    ...body,
    ...(movedModel ? { model: pin } : {}),
    // An effort belongs to the model it is run at, and the daemon drops it
    // from a pin whose model moves — so a model on the wire takes the effort
    // box with it, and what is stored is what the form said.
    ...(movedModel || movedEffort ? { effort: at } : {}),
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
    agents: [
      {
        seat: "author" as const,
        skills: parseSkills(values.author_skills),
        ...pinFields(values.author_model, values.author_effort),
      },
      ...reviewers(values),
    ],
    repo_id: repoId,
    depends_on: dependsOn(values),
    landing: values.landing,
  }
}
