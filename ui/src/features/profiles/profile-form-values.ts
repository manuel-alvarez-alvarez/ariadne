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
 * The briefing prompts are the other asymmetry. Create takes them inline, so
 * only the ones edited away from the role default go into the body; update has
 * no room for them at all — each changed kind is its own `PUT`, which is why
 * {@link changedPrompts} is separate from either request builder.
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
 * Which kinds a form holds is never decided here — the create dialog takes them
 * from the role defaults and the edit dialog from the profile's own prompts, in
 * the order the daemon sends them.
 */
export interface PromptFormValue {
  kind: PromptKind
  content: string
}

/**
 * Extra flags are held as objects because `useFieldArray` keys rows by
 * identity: an array of bare strings loses the row a value belongs to as soon
 * as one is removed.
 *
 * The briefings are an array for the same reason, and because their order is
 * the daemon's.
 */
export const profileFormSchema = z.object({
  name: z
    .string()
    .refine((value) => value.trim().length > 0, { message: "Give the profile a name." }),
  role: z.enum(ROLES),
  agentKind: z.enum(AGENT_KIND_CHOICES),
  model: z.string(),
  systemPrompt: z
    .string()
    .refine((value) => value.trim().length > 0, { message: "A system prompt is required." }),
  extraFlags: z.array(z.object({ value: z.string() })),
  // A briefing may legitimately be emptied, so there is nothing to validate.
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
    extraFlags: [],
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
    extraFlags: profile.extra_flags.map((value) => ({ value })),
    prompts: prompts.map((prompt) => ({ kind: prompt.kind, content: prompt.content })),
  }
}

/**
 * The prompts whose text is not what `baseline` holds.
 *
 * Both dialogs save through this: on create the baseline is the role's
 * defaults, so only edited briefings are sent and the rest are seeded by the
 * daemon; on edit it is what the daemon last answered, so an untouched briefing
 * is never written.
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
 * The create body, with only the briefings the user actually edited.
 *
 * `defaults` is what the role would seed on its own: a kind left at its default
 * is left out of the body entirely, which is what the daemon reads as "seed
 * this one yourself".
 */
export function toCreateRequest(
  values: ProfileFormValues,
  defaults: readonly PromptFormValue[] = [],
): CreateProfileRequest {
  const model = values.model.trim()
  const prompts = changedPrompts(values.prompts, defaults)
  return {
    name: values.name.trim(),
    role: values.role,
    // Create takes the absent value itself rather than a sentinel.
    agent_kind: values.agentKind === AUTO_AGENT_KIND ? null : values.agentKind,
    model: model.length > 0 ? model : null,
    system_prompt: values.systemPrompt,
    extra_flags: cleanFlags(values.extraFlags),
    ...(prompts.length > 0 ? { prompts } : {}),
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
    extra_flags: cleanFlags(values.extraFlags),
  }
}

/** Flags go on an argv line, so blank rows are dropped and edges trimmed. */
function cleanFlags(flags: ProfileFormValues["extraFlags"]): string[] {
  return flags.map((flag) => flag.value.trim()).filter((flag) => flag.length > 0)
}
