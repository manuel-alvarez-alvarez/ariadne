/**
 * The forge a repository's remote is on (spec 025), as the repositories screen
 * reads it and as its form writes it back.
 *
 * The daemon detects the forge itself, off the checkout's `origin`; what the
 * screen decides is only whether Ariadne works with it. What the two roles of
 * the integration run on is set from the CLI (`ariadne repo update`).
 */

import type { ForgeDto } from "@/api"

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

/** The forge CLI that signs Ariadne in to the host: `gh`, `glab`. */
export function forgeCliName(kind: ForgeDto["kind"]): string {
  return kind === "github" ? "gh" : "glab"
}
