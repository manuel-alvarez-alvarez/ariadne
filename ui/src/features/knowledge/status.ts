/**
 * The knowledge-base vocabulary (022): how each index state, interaction kind
 * and confidence is named and coloured, kept in one place the way every other
 * feature's status module is (see `@/features/tasks/status.ts`).
 */

import type { KnowledgeConfidence, KnowledgeInteractionKind, KnowledgeState } from "./types"

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

/** A total record over the daemon's own vocabulary — see `@/lib/format`'s `SEAT_LABELS`. */
export const INTERACTION_KIND_LABELS: Record<KnowledgeInteractionKind, string> = {
  depends_on: "Depends on",
  references: "References",
  calls_route: "Calls route",
  sets_env: "Sets env",
}

export const CONFIDENCE_LABELS: Record<KnowledgeConfidence, string> = {
  exact: "Exact",
  heuristic: "Heuristic",
}
