/**
 * The model field of the profile form: {@link ModelPicker}, scoped by the
 * agent select next to it.
 *
 * Every form that assigns an agent scopes the catalog to it; the profile form
 * is the one whose agent select has a choice that names no CLI at all, so it
 * is the one that ever asks for the catalog whole: a pinned agent gets its own
 * models, "Auto-resolve" gets the union with a heading per agent — and, unlike
 * a slot with no pin, still an open field, since a profile with no agent
 * pinned may well have a model.
 */

import type { ModelDto } from "@/api"

import { ModelPicker } from "./model-picker"
import { type AgentKindChoice, AUTO_AGENT_KIND } from "./profile-form-values"

export function ModelCombobox({
  value,
  onChange,
  agentKind,
  models,
}: {
  value: string
  onChange: (value: string) => void
  /** The agent select's current choice; scopes the options, never the value. */
  agentKind: AgentKindChoice
  /** The catalog, or undefined while it is loading or failed to load. */
  models: ModelDto[] | undefined
}) {
  return (
    <ModelPicker
      value={value}
      onChange={onChange}
      models={models}
      agentKind={agentKind === AUTO_AGENT_KIND ? undefined : agentKind}
    />
  )
}
