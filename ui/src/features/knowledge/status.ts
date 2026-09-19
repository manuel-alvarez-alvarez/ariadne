/**
 * The knowledge-base vocabulary (022): how each index state, interaction kind
 * and confidence is named and coloured, kept in one place the way every other
 * feature's status module is (see `@/features/tasks/status.ts`).
 */

import type { KnowledgeState } from "@/api"
import type { GraphTone } from "./graph/graph-model"
import type { KnowledgeConfidence, KnowledgeInteractionKind, KnowledgeStep } from "./types"

interface StateMeta {
  label: string
  /** Badge classes, from the status ramp in `index.css`. */
  badge: string
  dot: string
}

export const KNOWLEDGE_STATE_META: Record<KnowledgeState, StateMeta> = {
  idle: { label: "Idle", badge: "bg-status-done-soft text-status-done-fg", dot: "bg-status-done" },
  indexing: {
    label: "Indexing",
    badge: "bg-status-active-soft text-status-active-fg",
    dot: "bg-status-active",
  },
  failed: {
    label: "Failed",
    badge: "bg-status-danger-soft text-status-danger-fg",
    dot: "bg-status-danger",
  },
  disabled: {
    label: "Disabled",
    badge: "bg-muted text-muted-foreground",
    dot: "bg-muted-foreground/60",
  },
}

/** Every interaction kind, in the order the daemon lists them. */
export const INTERACTION_KINDS: readonly KnowledgeInteractionKind[] = [
  "depends_on",
  "references",
  "calls_route",
  "sets_env",
]

/** A total record over the daemon's own vocabulary — see `@/lib/format`'s `SEAT_LABELS`. */
const INTERACTION_KIND_LABELS: Record<KnowledgeInteractionKind, string> = {
  depends_on: "Depends on",
  references: "References",
  calls_route: "Calls route",
  sets_env: "Sets env",
}

/** The colour each kind of edge is drawn in: a step of the status ramp. */
export const INTERACTION_KIND_TONES: Record<KnowledgeInteractionKind, GraphTone> = {
  depends_on: "active",
  references: "review",
  calls_route: "warn",
  sets_env: "ready",
}

export const CONFIDENCE_LABELS: Record<KnowledgeConfidence, string> = {
  exact: "Exact",
  heuristic: "Heuristic",
}

/** What answered the name, read beside the confidence: "via same directory". */
const STEP_LABELS: Record<KnowledgeStep, string> = {
  file: "same file",
  directory: "same directory",
  import: "import",
  repository: "repository",
  path: "path",
  route: "route",
  name: "name",
}

/** The schema types these as strings; a value the screen does not know reads as written. */
export function interactionKindLabel(kind: string): string {
  return INTERACTION_KIND_LABELS[kind as KnowledgeInteractionKind] ?? kind
}

export function confidenceLabel(confidence: string): string {
  return CONFIDENCE_LABELS[confidence as KnowledgeConfidence] ?? confidence
}

export function stepLabel(step: string): string {
  return STEP_LABELS[step as KnowledgeStep] ?? step
}
