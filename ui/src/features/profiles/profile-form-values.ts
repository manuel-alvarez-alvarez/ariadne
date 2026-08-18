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
 */

import { z } from "zod"

import type { CreateProfileRequest, ProfileDto, Role, UpdateProfileRequest } from "@/api"

import { AGENT_KINDS, ROLES } from "./profile-labels"

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
 * Extra flags are held as objects because `useFieldArray` keys rows by
 * identity: an array of bare strings loses the row a value belongs to as soon
 * as one is removed.
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
  }
}

/** An existing profile as form values, for the edit dialog. */
export function profileToFormValues(profile: ProfileDto): ProfileFormValues {
  return {
    name: profile.name,
    role: profile.role,
    agentKind: profile.agent_kind ?? AUTO_AGENT_KIND,
    model: profile.model ?? "",
    systemPrompt: profile.system_prompt,
    extraFlags: profile.extra_flags.map((value) => ({ value })),
  }
}

export function toCreateRequest(values: ProfileFormValues): CreateProfileRequest {
  const model = values.model.trim()
  return {
    name: values.name.trim(),
    role: values.role,
    // Create takes the absent value itself rather than a sentinel.
    agent_kind: values.agentKind === AUTO_AGENT_KIND ? null : values.agentKind,
    model: model.length > 0 ? model : null,
    system_prompt: values.systemPrompt,
    extra_flags: cleanFlags(values.extraFlags),
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
