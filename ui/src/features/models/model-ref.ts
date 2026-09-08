/**
 * The one string a model is chosen by, and everything the screens do with it:
 * `<agent_kind>:<model>` — the agent CLI, and after a `:` one model of it.
 *
 * The agent half is structure and the model half is free text the CLI is
 * handed as typed, so what splits the two is the *first* colon and never a
 * later one: `opencode:ollama/llama3:8b` is that opencode id whole, tag and
 * all. Both halves are required, and a model that names no CLI has no spelling
 * at all — the daemon refuses it by name, and {@link modelRefError} refuses it
 * here first, so a typo is a field error rather than a round trip.
 *
 * This is the client's mirror of `ariadne_core::ModelRef`, and the daemon
 * stays the authority: the check here only knows the three agent CLIs, and
 * everything after the colon is passed on untouched.
 *
 * Sessions are the one place the pair is still two fields on the wire — a
 * session records the CLI it was launched on and the model beside it — so
 * {@link formatModelRef} composes theirs, and one badge serves both.
 */

import { z } from "zod"

import type { AgentKind } from "@/api"

import { AGENT_KINDS } from "./labels"

/**
 * The agent CLI a string names, in either spelling: the wire one
 * (`claude_code`) and the hyphenated one a person types (`claude-code`) name
 * the same CLI, exactly as the daemon reads them.
 */
function agentKind(raw: string): AgentKind | undefined {
  const wire = raw.replace(/-/g, "_")
  return AGENT_KINDS.find((kind) => kind === wire)
}

/** The agent CLIs a refusal lists, in the order everything lists them. */
function kinds(): string {
  return AGENT_KINDS.join(", ")
}

/** The two halves as one id: `claude_code:claude-opus-5`. */
export function formatModelRef(kind: AgentKind, model: string): string {
  return `${kind}:${model}`
}

/**
 * Why this text is not a model reference, or null where it is one — the
 * daemon's own rule, said on the field rather than after a failed submit.
 *
 * A model is required wherever a pin is written.
 */
export function modelRefError(text: string): string | null {
  const trimmed = text.trim()
  if (trimmed.length === 0) return "Choose a model."
  const colon = trimmed.indexOf(":")
  if (colon < 0) {
    if (agentKind(trimmed)) return `Nothing after the agent CLI — write "${trimmed}:<model>".`
    return `"${trimmed}" names no agent CLI — write claude_code:${trimmed}, or whichever of ${kinds()} runs it.`
  }
  const agent = trimmed.slice(0, colon)
  if (!agentKind(agent)) {
    return `"${agent}" is no agent CLI — what stands before the ":" is one of ${kinds()}.`
  }
  if (trimmed.length === colon + 1) {
    return "Choose a model."
  }
  return null
}

/** The model field as every form that assigns one validates it. */
export function modelRefField() {
  return z.string().superRefine((text, ctx) => {
    const message = modelRefError(text)
    if (message) ctx.addIssue({ code: z.ZodIssueCode.custom, message })
  })
}

/**
 * The two halves of a reference, or null where the text is not one: the agent
 * CLI it names, and the model of it after the `:`.
 *
 * The split is {@link modelRefError}'s, said once: what an effort can be run
 * at is a question about the model half, and the picker beside a model box has
 * to ask it of whatever is typed there.
 */
export function parseModelRef(text: string): { agentKind: AgentKind; model: string } | null {
  const trimmed = text.trim()
  if (trimmed.length === 0 || modelRefError(trimmed) !== null) return null
  const colon = trimmed.indexOf(":")
  const kind = agentKind(trimmed.slice(0, colon))
  return kind ? { agentKind: kind, model: trimmed.slice(colon + 1) } : null
}

/**
 * A pin as a screen shows it: the model, and after an `@` the effort it is run
 * at where one is pinned.
 */
export function pinLabel(model: string, effort: string | null | undefined): string {
  return effort && effort.length > 0 ? `${model} @ ${effort}` : model
}
