/**
 * How a value is spelled for the screen.
 *
 * Everything here is app-wide vocabulary rather than any one feature's: an id
 * is shortened the same way on the board and in a panel, a timestamp reads the
 * same in a table and in a tooltip, and a role is called the same thing in the
 * profiles table, the session lists and the message threads. Written per
 * feature, these drifted; written once, they cannot.
 *
 * Formatting only, and pure. *Which* form a screen shows, when it re-renders
 * and where the exact stamp hangs off it is one decision made once, in
 * {@link import("@/components/when").When}.
 */

import { type ClassValue, clsx } from "clsx"
import { twMerge } from "tailwind-merge"

import {
  type AgentKind,
  ApiError,
  type AuthorRole,
  HTTP_ERROR_CODE,
  type Role,
  type TokenUsage,
} from "@/api"

/**
 * The language every formatted value is spelled in, pinned rather than taken
 * from the system: these strings sit in the middle of English sentences
 * ("updated 3 minutes ago", "in 1.2M (cached 1.1M)"), and a machine set to
 * another language would produce half-translated lines.
 */
const LOCALE = "en"

/** Tailwind class lists composed and de-conflicted; what every component builds its `className` with. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// ── Counts ────────────────────────────────────────────────────────────────

/**
 * "1 task", "3 tasks" — the count and its noun, agreeing.
 *
 * The regular `-s` plural is the default because almost every noun this app
 * counts takes it: tasks, items, verdicts, profiles. The one that does not —
 * "repositories" — passes its plural in.
 */
export function plural(count: number, noun: string, many = `${noun}s`): string {
  return `${count} ${count === 1 ? noun : many}`
}

// ── Tokens ────────────────────────────────────────────────────────────────

/**
 * Token counts run to seven digits, and every surface that shows one is a
 * table cell, a card header or a 48rem panel column: `1234567` is both
 * unreadable and wider than the space it has. The compact form keeps three
 * significant digits, which is as much as a reader compares by — `12.3k` next
 * to `1.2M` says everything a row is read for, and the exact number is one
 * hover away wherever this is shown.
 *
 * Character for character what the CLI's own `tokens` prints (see
 * `crates/ariadne-cli/src/output.rs`): the same count has to read the same in
 * a terminal and on a screen, or the two look like they disagree about a
 * number neither of them is wrong about. That is what the decimal a scaled
 * figure always carries is for — `1.0M`, not the `1M` that would otherwise sit
 * in a column under `999.9k` looking like a different kind of value.
 */
export function formatTokens(count: number): string {
  const total = Math.max(0, Math.round(count))
  if (total < 1_000) return String(total)
  // Rounded to one decimal *before* the unit is picked, so a count that rounds
  // up into the next one is spelled in it: 999,960 is `1.0M`, never `1000.0k`.
  const thousands = Math.round(total / 100) / 10
  if (thousands < 1_000) return `${thousands.toFixed(1)}k`
  return `${(Math.round(total / 100_000) / 10).toFixed(1)}M`
}

const EXACT = new Intl.NumberFormat(LOCALE)

/**
 * What an agent spent, as one line: `in 1.2M (cached 1.1M) · out 45.3k`.
 *
 * Cached input is a subset of the input beside it rather than a fourth number
 * to add up, which is why it reads as a parenthesis on that half. It is shown
 * even at zero: a run with no cache hits is a fact about the run, and a
 * parenthesis that comes and goes down a column is harder to read than one
 * that says `0`.
 *
 * `exact` spells the same sentence with every digit, for the hint behind a
 * compact figure: the rounding must not be the only form a reader can get at,
 * since two counts a hundred thousand apart both round to `1.2M`.
 */
export function usageSummary(usage: TokenUsage, { exact = false } = {}): string {
  const spell = exact ? (count: number) => EXACT.format(count) : formatTokens
  return `in ${spell(usage.input_tokens)} (cached ${spell(usage.cached_input_tokens)}) · out ${spell(usage.output_tokens)}`
}

// ── Identifiers ───────────────────────────────────────────────────────────

/**
 * Ids are 26-character ULIDs: unreadable in full, but the tail is enough to
 * tell two of them apart. The full id always stays one hint or one copy away
 * wherever this is shown — see {@link import("@/components/copyable-id").CopyableId}.
 */
export function shortId(id: string): string {
  return id.length <= 10 ? id : `…${id.slice(-8)}`
}

/** Git object ids are shown the way git shows them. */
export function shortSha(sha: string): string {
  return sha.slice(0, 10)
}

/** How much of a separator-less value is worth keeping, as in {@link shortId}. */
const TAIL_CHARS = 8

/**
 * Splits a value into the part that may be truncated and the part that must
 * survive. The two concatenated are always the value itself, so a renderer can
 * shrink the head and still show the whole thing when there is room.
 *
 * Cutting the end is the browser's default and the right one for most things.
 * It is the wrong one for values whose *tail* is what identifies them — a
 * worktree path ends in the task's own directory, a branch ends in the tail of
 * the task id — so an ellipsis at the end keeps the part no one can read and
 * drops the part a person recognises.
 */
export function middleTruncate(value: string): { head: string; tail: string } {
  const cut = value.lastIndexOf("/")
  // The last segment is only a name worth keeping whole while it is the
  // *smaller* part: `~/.ariadne/worktrees/<goal>/<task>-eng` ends in a name,
  // while a value that is mostly its own last segment — `feature/<a long
  // branch name>` — would lose that segment's tail to the split, which is the
  // half that tells two of them apart. It falls through to the character
  // split below instead.
  if (cut > 0 && cut < value.length - 1 && value.length - cut <= cut) {
    return { head: value.slice(0, cut), tail: value.slice(cut) }
  }
  if (value.length <= TAIL_CHARS) return { head: value, tail: "" }
  return { head: value.slice(0, -TAIL_CHARS), tail: value.slice(-TAIL_CHARS) }
}

// ── The daemon's vocabulary ───────────────────────────────────────────────

/**
 * Both maps are total records over the generated enums, so a new role or agent
 * CLI in the daemon fails to compile here until it is given a name.
 */
export const ROLE_LABELS: Record<Role, string> = {
  planner: "Planner",
  engineer: "Engineer",
  reviewer: "Reviewer",
}

/**
 * A message author is a role plus the two speakers that run no session: the
 * person at the keyboard, and the daemon itself.
 */
export const AUTHOR_ROLE_LABELS: Record<AuthorRole, string> = {
  ...ROLE_LABELS,
  user: "You",
  system: "System",
}

export const AGENT_KIND_LABELS: Record<AgentKind, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  opencode: "OpenCode",
}

// ── Failures ──────────────────────────────────────────────────────────────

/**
 * What to put in front of the user when a call fails.
 *
 * The daemon's error code is what its docs and the CLI talk about, so it is
 * shown alongside the message rather than swallowed — except for the two codes
 * that say nothing a reader could look up: a request that never reached the
 * daemon, and a non-2xx that carried no envelope.
 */
export function describeError(error: unknown): string {
  if (!ApiError.is(error)) return error instanceof Error ? error.message : String(error)
  return error.isNetworkError || error.code === HTTP_ERROR_CODE
    ? error.message
    : `${error.message} (${error.code})`
}

// ── Time ──────────────────────────────────────────────────────────────────

// The daemon stamps everything with RFC 3339 in UTC; the user reads it in
// their own zone. Anything unparsable is passed through as-is rather than
// rendered as "Invalid Date". The time zone comes from the system, which is
// the part of the reader's locale that matters here; the language does not —
// see LOCALE at the top of the file.

/** What a nullish timestamp renders as, so a column never goes blank. */
const NONE = "—"

const ABSOLUTE = new Intl.DateTimeFormat(LOCALE, { dateStyle: "medium", timeStyle: "short" })

const RELATIVE = new Intl.RelativeTimeFormat(LOCALE, { numeric: "auto" })

/** Units of the "3 hours ago" form, largest first, in seconds. */
const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
]

function parseDate(iso: string): Date | null {
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? null : date
}

/** `16 Aug 2026, 14:03` — the full stamp, for tooltips and metadata tables. */
export function formatAbsolute(iso: string | null | undefined): string {
  if (!iso) return NONE
  const date = parseDate(iso)
  return date ? ABSOLUTE.format(date) : iso
}

/** `3 minutes ago` — the glanceable form; the absolute stamp is the hint behind it. */
export function formatRelative(iso: string, now: number = Date.now()): string {
  const date = parseDate(iso)
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
 * carries no "ago" suffix. Nothing renders this itself:
 * {@link import("@/components/when").When} does, with `format="age"`.
 */
export function formatAge(iso: string | null | undefined, now: number): string {
  if (!iso) return NONE
  const date = parseDate(iso)
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
