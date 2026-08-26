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
 * What each agent runs on is the one thing with a meaning of its own here, and
 * it is a pair: an agent CLI, and a model narrowing it. Picking no agent — the
 * `"default"` choice of {@link AgentPin} — is the slot on its profile's own
 * agent and model both, and an agent with no model is that CLI's own default.
 * So a slot only carries a pin where it runs on something *else* than its
 * profile, which is what {@link taskToFormValues} seeds and what the two
 * request builders send:
 *
 * - on create, an agent is sent with the model beside it when the box holds
 *   one, and a slot with no agent is sent with neither field;
 * - on edit, `UpdateTaskRequest.agent_kind` is a tri-state — absent leaves the
 *   pins alone, `"default"` puts them back on the profile's — so an untouched
 *   pair sends nothing and one taken off sends the sentinel. Which of the two
 *   it is can only be answered against the pair the form was *seeded* with,
 *   hence the `initial` argument.
 *
 * Reviewers are sent whole either way, since the list replaces the task's, so
 * no baseline is needed there: each row goes out as the controls on screen
 * read, which is the reading the user saw. On a cold open that means a pin is
 * re-sent rather than dropped — the safe way round, since a slot sent without
 * an agent is one put back on its profile's.
 */

import { z } from "zod"

import type { AgentKind, CreateTaskRequest, ProfileDto, TaskDto, UpdateTaskRequest } from "@/api"
import {
  AGENT_PIN_CHOICES,
  type AgentPin,
  PROFILE_AGENT_KIND,
  pinnedAgentKind,
} from "@/features/profiles/agent-pin"

export function makeTaskFormSchema(opts: { creating: boolean; requireRepo: boolean }) {
  return z.object({
    title: z.string().trim().min(1, "Give the task a title."),
    description: z.string(),
    engineer_profile: opts.creating ? z.string().min(1, "Choose an engineer profile.") : z.string(),
    engineer_agent: z.enum(AGENT_PIN_CHOICES),
    // Free text: the catalog is a suggestion, and an id it does not carry is
    // handed to the chosen CLI as typed.
    engineer_model: z.string(),
    reviewers: z
      .array(
        z.object({
          profile: z.string().min(1, "Choose a reviewer profile."),
          agent: z.enum(AGENT_PIN_CHOICES),
          model: z.string(),
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

/** What one slot runs on, as its two controls hold it. */
interface Pin {
  agent: AgentPin
  model: string
}

/** The slot on its profile's own agent and model, which is where a form starts. */
const NO_PIN: Pin = { agent: PROFILE_AGENT_KIND, model: "" }

const CREATE_DEFAULTS: TaskFormValues = {
  title: "",
  description: "",
  engineer_profile: "",
  engineer_agent: NO_PIN.agent,
  engineer_model: NO_PIN.model,
  reviewers: [{ profile: "", ...NO_PIN }],
  repo_id: "",
  depends_on: [],
}

/**
 * One slot's controls: the pin, unless it is only repeating what the profile
 * already runs on.
 *
 * The pair is what is compared, not the model alone: a task pinned to a CLI
 * with no model of its own runs that CLI's default, which is a different thing
 * from the profile's model even where both name the same agent. A pin that agrees
 * with the profile on both halves shows as no pin at all — which is also what
 * leaves an untouched form saying nothing about either field. With the
 * profiles not loaded yet there is nothing to compare against, so the pin is
 * shown as it stands.
 */
function overrideOf(
  agentKind: AgentKind | null | undefined,
  model: string | null | undefined,
  profile: ProfileDto | undefined,
): Pin {
  if (!agentKind) return NO_PIN
  const same = profile?.agent_kind === agentKind && (profile?.model ?? null) === (model ?? null)
  return same ? NO_PIN : { agent: agentKind, model: model ?? "" }
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
  const engineer = overrideOf(task.agent_kind, task.model, profileOf(task.engineer_profile_id))
  return {
    title: task.title,
    description: task.description,
    engineer_profile: task.engineer_profile_id,
    engineer_agent: engineer.agent,
    engineer_model: engineer.model,
    reviewers: task.reviewers.map((reviewer) => ({
      profile: reviewer.profile_id,
      ...overrideOf(reviewer.agent_kind, reviewer.model, profileOf(reviewer.profile_id)),
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
 * The two pin fields of a slot that is assigned one, or nothing at all where
 * it runs on its profile's own. A chosen agent with an empty box is the agent
 * alone, which the daemon reads as that CLI's default model.
 */
function pinFields(pin: Pin): { agent_kind?: AgentKind; model?: string } {
  const agentKind = pinnedAgentKind(pin.agent)
  if (!agentKind) return {}
  const model = pin.model.trim()
  return { agent_kind: agentKind, ...(model.length > 0 ? { model } : {}) }
}

/** Each slot with what it runs on, or with nothing — which is its profile's. */
function reviewers(values: TaskFormValues): CreateTaskRequest["reviewers"] {
  return values.reviewers.map((row) => ({ profile: row.profile, ...pinFields(row) }))
}

/**
 * `PATCH /v1/tasks/{id}` carries neither the engineer nor the repository.
 *
 * `initial` is the engineer's agent and model as the form was last seeded —
 * what the user found in them. It is what says whether a slot reading "the
 * profile's own" was put back there, which asks for the sentinel, or was there
 * all along, which asks for nothing. It has to be the *seeded* pair rather
 * than what {@link taskToFormValues} would answer now: the seeding depends on
 * profiles that arrive after the dialog opens, and a form the user has already
 * typed in is deliberately not re-seeded, so the two part ways — and controls
 * nobody touched would go on the wire as a pin the user never chose.
 */
export function toUpdateTaskRequest(values: TaskFormValues, initial: Pin): UpdateTaskRequest {
  const pin: Pin = { agent: values.engineer_agent, model: values.engineer_model.trim() }
  const body: UpdateTaskRequest = {
    title: values.title.trim(),
    description: values.description,
    reviewers: reviewers(values),
    depends_on: dependsOn(values),
  }
  if (pin.agent === initial.agent && pin.model === initial.model.trim()) return body
  // Taken off its pin: the sentinel alone, which is the profile's agent *and*
  // model. Otherwise the pin as it reads, model included where it has one.
  if (pin.agent === PROFILE_AGENT_KIND) return { ...body, agent_kind: PROFILE_AGENT_KIND }
  return { ...body, ...pinFields(pin) }
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
    ...pinFields({ agent: values.engineer_agent, model: values.engineer_model }),
    reviewers: reviewers(values),
    repo_id: repoId,
    depends_on: dependsOn(values),
  }
}
