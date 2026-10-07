/**
 * The forge a repository's remote is on (spec 025), as the repositories screen
 * reads it and as its form writes it back.
 *
 * The daemon detects the forge itself, off the checkout's `origin`; what the
 * user decides is only whether Ariadne works with it, and what the two roles
 * of the integration run on. A role with no model starts no session.
 */

import type { ForgeDto, ForgeUpdate } from "@/api"

const KIND_LABELS: Record<ForgeDto["kind"], string> = {
  github: "GitHub",
  gitlab: "GitLab",
}

/** `GitHub`, `GitLab`. */
export function forgeKindLabel(kind: ForgeDto["kind"]): string {
  return KIND_LABELS[kind]
}

/** The forge repository, as its own URL would name it: `github.com/acme/widgets`. */
export function forgeRepositoryLabel(forge: ForgeDto): string {
  return `${forge.host}/${forge.owner}/${forge.name}`
}

/** What the form holds of the integration: the switch and the two pins. */
interface ForgeValues {
  forge_enabled: boolean
  babysit_model: string
  babysit_effort: string
  review_model: string
  review_effort: string
}

/** The form's values for a stored integration: no pin is the empty string. */
export function forgeValues(forge: ForgeDto | null | undefined): ForgeValues {
  return {
    forge_enabled: forge?.enabled ?? false,
    babysit_model: forge?.babysit_model ?? "",
    babysit_effort: forge?.babysit_effort ?? "",
    review_model: forge?.review_model ?? "",
    review_effort: forge?.review_effort ?? "",
  }
}

/**
 * What changed against the stored integration, or null where nothing did.
 *
 * Only a change is sent: an absent field is one the daemon leaves alone, and a
 * pin sent unchanged would be checked again against a catalog that may have
 * turned its model off since. A role's model and effort travel together, and
 * an empty model is how the daemon spells "no pin".
 */
export function forgeChanges(forge: ForgeDto, values: ForgeValues): ForgeUpdate | null {
  const stored = forgeValues(forge)
  const update: ForgeUpdate = {}
  if (values.forge_enabled !== stored.forge_enabled) update.enabled = values.forge_enabled
  if (
    values.babysit_model !== stored.babysit_model ||
    values.babysit_effort !== stored.babysit_effort
  ) {
    update.babysit_model = values.babysit_model
    update.babysit_effort = values.babysit_effort
  }
  if (
    values.review_model !== stored.review_model ||
    values.review_effort !== stored.review_effort
  ) {
    update.review_model = values.review_model
    update.review_effort = values.review_effort
  }
  return Object.keys(update).length > 0 ? update : null
}
