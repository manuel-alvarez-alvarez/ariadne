/**
 * The one meaning each colour on the Stats screen carries, off the status ramp
 * in `index.css`. Every panel's chart builds its own series config out of
 * these five, so "failed" is the same red in the models chart and in the
 * outcomes chart rather than each picking its own.
 *
 * Read through the CSS variable rather than a literal value, so a chart
 * drawn in the dark theme reads the dark theme's ramp with no switch of its
 * own.
 */
export const STATUS_COLORS = {
  /** A run or a task that ended the way it was meant to. */
  done: "var(--color-status-done)",
  /** A run or a task that failed, or an answer that denied. */
  danger: "var(--color-status-danger)",
  /** A run flagged stalled. */
  warn: "var(--color-status-warn)",
  /** A task cancelled, or an answer cancelled. */
  pending: "var(--color-status-pending)",
  /** A plain count with no status of its own. */
  active: "var(--color-status-active)",
} as const
