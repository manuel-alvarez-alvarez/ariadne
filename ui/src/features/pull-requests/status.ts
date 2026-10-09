/**
 * How a request's checks and review decision read as pills, in the table
 * and in its panel alike. `none` is no pill at all: there is nothing to say.
 */

export const CHECKS: Record<string, { label: string; tone: string; dot: string } | undefined> = {
  success: {
    label: "Passing",
    tone: "bg-status-done-soft text-status-done-fg",
    dot: "bg-status-done",
  },
  failure: {
    label: "Failing",
    tone: "bg-status-danger-soft text-status-danger-fg",
    dot: "bg-status-danger",
  },
  pending: {
    label: "Running",
    tone: "bg-status-active-soft text-status-active-fg",
    dot: "bg-status-active",
  },
}

export const REVIEW: Record<string, { label: string; tone: string } | undefined> = {
  approved: { label: "Approved", tone: "bg-status-done-soft text-status-done-fg" },
  changes_requested: {
    label: "Changes requested",
    tone: "bg-status-danger-soft text-status-danger-fg",
  },
  review_required: { label: "Review required", tone: "bg-status-warn-soft text-status-warn-fg" },
}

/** The pill an Ariadne review of the request wears while it is asked. */
export const ARIADNE_REVIEWING = {
  label: "Ariadne reviewing",
  tone: "bg-status-review-soft text-status-review-fg",
}
