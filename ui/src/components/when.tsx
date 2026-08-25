/**
 * "When did this happen", said the same way everywhere.
 *
 * Four rules, and they are the reason this is a component rather than a call
 * to `lib/time.ts` at each site:
 *
 * - **One format.** Relative text ("3 minutes ago") on every card, row and
 *   panel; the exact stamp is the hint behind it, never the label. Tables that
 *   are read down a column are the one exception — see {@link WhenProps.format}.
 * - **It stays true.** The text re-renders on the shared clock
 *   ({@link useNow}), so a card left open does not go on claiming "2 minutes
 *   ago" an hour later. Nothing here polls; one interval drives every
 *   timestamp on screen.
 * - **The stamp is reachable.** It rides in a real `Tooltip`, which opens on
 *   focus as well as on hover — a `title=` is a mouse-only hint, and this is
 *   the app's one piece of metadata that every screen shows.
 * - **It is a `<time>`.** `dateTime` carries the machine-readable instant, so
 *   what is rendered can be the loose form without losing the precise one.
 */

import type { ReactNode } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useNow } from "@/hooks/use-now"
import { cn, formatAbsolute, formatAge, formatRelative } from "@/lib/format"

type WhenProps = {
  /** RFC 3339 stamp from the daemon; nullish renders as a dash, with no hint. */
  at: string | null | undefined
  /**
   * `"relative"` — `3 minutes ago` — is the form for anything read one at a
   * time: cards, strip rows, panel facts.
   *
   * `"age"` — `4m` — is for a column read down: the heading above it already
   * says what the number is the age of, and a table of "N minutes ago" is a
   * column of repeated words. It is the same clock and the same hint either
   * way; only the text is shorter.
   */
  format?: "relative" | "age"
  /** What the stamp is, prefixed to it in the hint ("updated 16 Aug 2026, 14:03"). */
  label?: string
  /** Further lines for the hint — the sibling stamps a row has no room for. */
  detail?: ReactNode
  className?: string
}

export function When({ at, format = "relative", label, detail, className }: WhenProps) {
  const now = useNow()

  // Nothing to point at, so nothing to hover or focus: an empty hint on a dash
  // is a focus stop that says "—".
  if (!at) return <span className={cn("text-muted-foreground", className)}>—</span>

  return (
    <Tooltip>
      <TooltipTrigger render={<time className={className} dateTime={at} />}>
        {format === "age" ? formatAge(at, now) : formatRelative(at, now)}
      </TooltipTrigger>
      <TooltipContent className={detail ? "flex-col items-start gap-0.5" : undefined}>
        <span>
          {label ? `${label} ` : ""}
          {formatAbsolute(at)}
        </span>
        {detail}
      </TooltipContent>
    </Tooltip>
  )
}

/** A further line of a {@link When} hint, in the hint's own muted face. */
export function WhenDetail({ label, at }: { label: string; at: string | null | undefined }) {
  return (
    <span className="text-background/70">
      {label} {formatAbsolute(at)}
    </span>
  )
}
