/**
 * Small formatting helpers for the task screens. The daemon stamps everything
 * with RFC 3339 in UTC; the user reads it in their own zone.
 *
 * The locale is pinned rather than taken from the system: these strings sit in
 * the middle of English sentences ("updated 3 minutes ago"), and a machine set
 * to another language would otherwise produce half-translated lines. The time
 * zone still comes from the system, which is the part that matters.
 */

import { ApiError } from "@/api"

const LOCALE = "en"

const ABSOLUTE = new Intl.DateTimeFormat(LOCALE, {
  dateStyle: "medium",
  timeStyle: "short",
})

const RELATIVE = new Intl.RelativeTimeFormat(LOCALE, { numeric: "auto" })

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
]

/** `16 Aug 2026, 14:03` — the full stamp, for tooltips and timelines. */
export function formatAbsolute(iso: string): string {
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? iso : ABSOLUTE.format(date)
}

/** `3 minutes ago` — the glanceable form, with the absolute stamp on hover. */
export function formatRelative(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso
  const seconds = (date.getTime() - Date.now()) / 1000
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return RELATIVE.format(Math.round(seconds / size), unit)
  }
  return RELATIVE.format(Math.round(seconds), "second")
}

/**
 * Ids are 26-character ULIDs: unreadable in full, but the tail is enough to
 * tell two of them apart. The full id always stays available as a `title`.
 */
export function shortId(id: string): string {
  return id.length <= 10 ? id : `…${id.slice(-8)}`
}

/** Git object ids are shown the way git shows them. */
export function shortSha(sha: string): string {
  return sha.slice(0, 10)
}

/**
 * What to put in front of the user when a call fails. The daemon's error code
 * is what its docs and the CLI talk about, so it is shown alongside the
 * message rather than swallowed.
 */
export function describeError(error: unknown): string {
  if (!ApiError.is(error)) return error instanceof Error ? error.message : String(error)
  return error.isNetworkError ? error.message : `${error.message} (${error.code})`
}
