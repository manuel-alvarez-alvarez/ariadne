/**
 * Timestamp formatting for the goal screens. The daemon sends RFC 3339 with
 * millisecond precision; everything below degrades to the raw string rather
 * than showing "Invalid Date" if that ever stops being true.
 */

const ABSOLUTE = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
})

const RELATIVE = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" })

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 60 * 60 * 1000],
  ["month", 30 * 24 * 60 * 60 * 1000],
  ["day", 24 * 60 * 60 * 1000],
  ["hour", 60 * 60 * 1000],
  ["minute", 60 * 1000],
]

function parse(timestamp: string): Date | null {
  const date = new Date(timestamp)
  return Number.isNaN(date.getTime()) ? null : date
}

/** e.g. `16 Aug 2026, 14:03`. */
export function formatAbsolute(timestamp: string): string {
  const date = parse(timestamp)
  return date ? ABSOLUTE.format(date) : timestamp
}

/** e.g. `3 hours ago`, falling back to the absolute form for anything odd. */
export function formatRelative(timestamp: string, now: number = Date.now()): string {
  const date = parse(timestamp)
  if (!date) return timestamp
  const elapsed = date.getTime() - now
  for (const [unit, ms] of UNITS) {
    if (Math.abs(elapsed) >= ms) return RELATIVE.format(Math.round(elapsed / ms), unit)
  }
  return "just now"
}
