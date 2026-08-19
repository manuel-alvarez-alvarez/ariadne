/**
 * Every timestamp the UI shows, formatted in one place.
 *
 * The daemon stamps everything with RFC 3339 in UTC; the user reads it in
 * their own zone. Anything unparsable is passed through as-is rather than
 * rendered as "Invalid Date".
 *
 * The locale is pinned rather than taken from the system: these strings sit in
 * the middle of English sentences ("updated 3 minutes ago"), and a machine set
 * to another language would otherwise produce half-translated lines. The time
 * zone still comes from the system, which is the part that matters.
 */

const LOCALE = "en"

/** What a nullish timestamp renders as, so a column never goes blank. */
const NONE = "—"

const ABSOLUTE = new Intl.DateTimeFormat(LOCALE, {
  dateStyle: "medium",
  timeStyle: "short",
})

const RELATIVE = new Intl.RelativeTimeFormat(LOCALE, { numeric: "auto" })

/** Units of the "3 hours ago" form, largest first, in seconds. */
const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
]

function parse(iso: string): Date | null {
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? null : date
}

/** `16 Aug 2026, 14:03` — the full stamp, for tooltips and metadata tables. */
export function formatAbsolute(iso: string | null | undefined): string {
  if (!iso) return NONE
  const date = parse(iso)
  return date ? ABSOLUTE.format(date) : iso
}

/** `3 minutes ago` — the glanceable form, with the absolute stamp on hover. */
export function formatRelative(iso: string, now: number = Date.now()): string {
  const date = parse(iso)
  if (!date) return iso
  const seconds = (date.getTime() - now) / 1000
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return RELATIVE.format(Math.round(seconds / size), unit)
  }
  return RELATIVE.format(Math.round(seconds), "second")
}

/**
 * Compact age, e.g. `12s`, `4m`, `3h`, `2d` — the tabular form, where the
 * label next to it ("Last activity", "Started") already says what it is, so it
 * carries no "ago" suffix. Pass `now` from
 * {@link import("@/features/sessions/use-now").useNow} so it refreshes itself.
 */
export function formatAge(iso: string | null | undefined, now: number): string {
  if (!iso) return NONE
  const date = parse(iso)
  if (!date) return iso
  return formatDuration((now - date.getTime()) / 1000)
}

/**
 * A span of seconds in the same compact form, for durations that are not the
 * age of a timestamp (the daemon's uptime).
 *
 * Every unit is floored, never rounded: 89 seconds is "1m", not the "2m" a
 * rounding step would jump to a second early.
 */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds))
  if (total < 60) return `${total}s`
  const minutes = Math.floor(total / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  return `${Math.floor(hours / 24)}d`
}
