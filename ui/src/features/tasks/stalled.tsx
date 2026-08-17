/**
 * "Stalled", wherever it is said.
 *
 * A stall is not a status: the task keeps whatever status it is parked in and
 * carries this flag on top of it, which is why it is not in
 * {@link import("./status").TASK_STATUS_META}. It was, however, being said
 * four different ways in three different colours — so the word, the
 * explanation and the step of the ramp live here, and the board card, the
 * panel header and the attention list all read them.
 *
 * The step is the warn one, the same `changes_requested` takes: the ramp's
 * warm end is what "something is wrong" looks like in this app, and it carries
 * dark mode, which the hand-rolled ambers it replaces did not.
 */

import { StatusBadge } from "@/components/status-badge"

export const STALLED_META = {
  label: "Stalled",
  hint: "The agent went idle without advancing the task.",
  /** Pill classes, for the surfaces that show it as a badge. */
  badge: "bg-status-warn-soft text-status-warn-fg",
  /** Text colour, for the board card, where the flag is a line and not a pill. */
  text: "text-status-warn-fg",
  /** Card border, for the board card that outlines itself when its task stalls. */
  border: "border-status-warn/40",
} as const

/** The flag as a pill, for the panel header and the attention list. */
export function StalledBadge() {
  return (
    <StatusBadge
      box="badge"
      label={STALLED_META.label}
      tone={STALLED_META.badge}
      title={STALLED_META.hint}
    />
  )
}
