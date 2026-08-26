/**
 * The model field of the profile form: {@link ModelPicker}, scoped by the
 * agent select next to it.
 *
 * The profile form is the one place where the agent CLI is chosen by hand
 * rather than derived from the model, so it is the one place where the catalog
 * is narrowed: a pinned agent gets its own models, "Auto-resolve" gets the
 * union with a heading per agent. The picker says nothing about the agent
 * underneath the field here — the select above it already does.
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
