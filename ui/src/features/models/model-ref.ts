/**
 * The one string a model is chosen by, and everything the screens do with it:
 * `<agent>:<model>` — the id of an agent in the daemon's registry, and after a
 * `:` one model of it.
 *
 * The agent half is structure and the model half is free text the agent is
 * handed as typed, so what splits the two is the *first* colon and never a
 * later one: `opencode-acp:ollama/llama3:8b` is that model id whole, tag and
 * all. Both halves are required, and a model that names no agent has no
 * spelling at all — the daemon refuses it by name, and {@link modelRefError}
 * refuses it here first, so a typo is a field error rather than a round trip.
 *
 * This is the client's mirror of `ariadne_core::ModelRef`, and the daemon
 * stays the authority: which agents there are is its registry's answer, so the
 * check here is only the shape, and everything after the colon is passed on
 * untouched.
 */

import { z } from "zod"

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
    return `"${trimmed}" is one half — write <agent>:${trimmed}, or ${trimmed}:<model>.`
  }
  if (colon === 0) return `Nothing before the ":" — write <agent>${trimmed}.`
  if (trimmed.length === colon + 1) return "Choose a model."
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
 * it names, and the model of it after the `:`.
 *
 * The split is {@link modelRefError}'s, said once: what an effort can be run
 * at is a question about the model half, and the picker beside a model box has
 * to ask it of whatever is typed there.
 */
export function parseModelRef(text: string): { agent: string; model: string } | null {
  const trimmed = text.trim()
  if (modelRefError(trimmed) !== null) return null
  const colon = trimmed.indexOf(":")
  return { agent: trimmed.slice(0, colon), model: trimmed.slice(colon + 1) }
}

/**
 * A pin as a screen shows it: the model, and after an `@` the effort it is run
 * at where one is pinned.
 */
export function pinLabel(model: string, effort: string | null | undefined): string {
  return effort && effort.length > 0 ? `${model} @ ${effort}` : model
}
