/**
 * The profile form's own value shape, and its translation to the daemon's two
 * request bodies.
 *
 * The part worth being careful about is the update. `UpdateProfileRequest`
 * leaves absent fields unchanged and clears the two optional ones through
 * *sentinel strings*: `agent_kind: "auto"` puts the profile back on
 * first-installed-CLI resolution, `model: "default"` (or empty) back on the
 * agent's own default. The form never asks anyone to type those — "Auto-resolve"
 * is an option in the agent select and an empty model box means the default —
 * so this module is the one place where those choices become the sentinels.
 *
 * `role` is deliberately missing from the update body: the daemon has no way to
 * change a profile's role after creation.
 *
 * The prompts are the other asymmetry. Neither body carries a briefing: a
 * changed kind is its own `PUT`, which is why {@link changedPrompts} is
 * separate from either request builder. The system prompt does travel with the
 * profile, and on create an empty box means "no prompt of its own", which is
 * what leaves the profile on the default of its role.
 */

import { z } from "zod"

import type {
  CreateProfileRequest,
  ProfileDto,
  PromptKind,
  Role,
  UpdateProfileRequest,
} from "@/api"

import { AGENT_KINDS, PROMPT_KINDS, ROLES } from "./profile-labels"

/**
 * The agent-kind choice standing for "no kind pinned". It is spelled like the
 * daemon's clear sentinel because that is exactly what it becomes on update.
 */
export const AUTO_AGENT_KIND = "auto"

/** What the daemon reads as "clear the model back to the agent's default". */
const DEFAULT_MODEL_SENTINEL = "default"

/** Every value the agent select offers: a real CLI, or auto-resolution. */
export const AGENT_KIND_CHOICES = [AUTO_AGENT_KIND, ...AGENT_KINDS] as const

export type AgentKindChoice = (typeof AGENT_KIND_CHOICES)[number]

/**
 * One briefing prompt while it is being edited: the daemon's kind and the text.
 *
 * Which kinds a form holds is never decided here — they are the ones the
 * profile's own prompts endpoint answers with, in the order it sends them, so a
 * new profile carries none until it exists.
 */
export interface PromptFormValue {
  kind: PromptKind
  content: string
}

/**
 * The briefings are an array because `useFieldArray` keys rows by identity,
 * and because their order is the daemon's.
 */
export const profileFormSchema = z.object({
  name: z
    .string()
    .refine((value) => value.trim().length > 0, { message: "Give the profile a name." }),
  role: z.enum(ROLES),
  agentKind: z.enum(AGENT_KIND_CHOICES),
  model: z.string(),
  // A prompt may legitimately be emptied — the daemon takes any text — and on
  // create an empty one means the role's default, so there is nothing to
  // validate.
  systemPrompt: z.string(),
  prompts: z.array(z.object({ kind: z.enum(PROMPT_KINDS), content: z.string() })),
})

export type ProfileFormValues = z.infer<typeof profileFormSchema>

/** A blank form, for the create dialog. */
export function emptyProfileFormValues(role: Role = "engineer"): ProfileFormValues {
  return {
    name: "",
    role,
    agentKind: AUTO_AGENT_KIND,
    model: "",
    systemPrompt: "",
    prompts: [],
  }
}

/**
 * An existing profile as form values, for the edit dialog.
 *
 * Its briefings arrive from their own endpoint, later than the profile itself,
 * so they are a second argument rather than a field of the DTO.
 */
export function profileToFormValues(
  profile: ProfileDto,
  prompts: readonly PromptFormValue[] = [],
): ProfileFormValues {
  return {
    name: profile.name,
    role: profile.role,
    agentKind: profile.agent_kind ?? AUTO_AGENT_KIND,
    model: profile.model ?? "",
    systemPrompt: profile.system_prompt,
    prompts: prompts.map((prompt) => ({ kind: prompt.kind, content: prompt.content })),
  }
}

/**
 * The prompts whose text is not what `baseline` holds.
 *
 * The baseline is what the daemon last answered, which is the default itself
 * for a prompt the profile has none of its own: an untouched briefing is never
 * written, so reading one does not quietly turn it into an override.
 */
export function changedPrompts(
  prompts: readonly PromptFormValue[],
  baseline: readonly PromptFormValue[],
): PromptFormValue[] {
  const before = new Map(baseline.map((prompt) => [prompt.kind, prompt.content]))
  return prompts
    .filter((prompt) => before.get(prompt.kind) !== prompt.content)
    .map((prompt) => ({ kind: prompt.kind, content: prompt.content }))
}

/**
 * The create body.
 *
 * A blank system prompt is sent as no system prompt at all, which is what
 * leaves the new profile on the default of its role — the same thing the CLI
 * does with a `create` that names no `system` prompt.
 */
export function toCreateRequest(values: ProfileFormValues): CreateProfileRequest {
  const model = values.model.trim()
  return {
    name: values.name.trim(),
    role: values.role,
    // Create takes the absent value itself rather than a sentinel.
    agent_kind: values.agentKind === AUTO_AGENT_KIND ? null : values.agentKind,
    model: model.length > 0 ? model : null,
    system_prompt: values.systemPrompt.trim().length > 0 ? values.systemPrompt : null,
  }
}

export function toUpdateRequest(values: ProfileFormValues): UpdateProfileRequest {
  const model = values.model.trim()
  return {
    name: values.name.trim(),
    // Both of these are sentinels when the user picked the "unset" choice.
    agent_kind: values.agentKind,
    model: model.length > 0 ? model : DEFAULT_MODEL_SENTINEL,
    system_prompt: values.systemPrompt,
  }
}
