/**
 * How many rows are behind a tab, once its list has arrived.
 *
 * Nothing while it has not: a count that starts at zero and jumps says the
 * list is empty for as long as the request takes. Zero itself is shown — "no
 * sessions yet" is an answer, and it is the one the tab is opened to find.
 *
 * A pill rather than a bare number, sized like {@link UnreadBadge} so the two
 * line up on one tab strip — and `secondary` where that one is filled with the
 * primary, because a total is background to a tab's label while what is new on
 * a thread is the thing being pointed at.
 *
 * The number alone would be read out as "Sessions 12", which says nothing
 * about what the twelve are, so the pill carries the noun for a screen reader
 * and the tab's own name still leads with its label.
 */

import { Badge } from "@/components/ui/badge"
import { plural } from "@/lib/format"

export function TabCount({ count, noun }: { count: number | undefined; noun: string }) {
  if (count === undefined) return null
  return (
    <Badge
      variant="secondary"
      className="h-4 px-1.5 text-[10px] tabular-nums"
      aria-label={plural(count, noun)}
    >
      {count > 99 ? "99+" : count}
    </Badge>
  )
}
