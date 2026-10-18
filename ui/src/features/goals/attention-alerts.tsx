/**
 * What needs attention, said everywhere the strip is not.
 *
 * "An agent is waiting on you" is the one thing this app must never let go
 * unnoticed, and the strip above the board only says it to someone already
 * looking at the board. So the count is lifted into the shell — the badge on
 * the Goals entry ({@link AttentionBadge}) and the window title, both of them
 * readable out of the corner of an eye and from another window — and anything
 * that becomes stuck while the user is on another screen raises one toast, with
 * the way to answer it on it.
 *
 * In-app only, deliberately: no desktop or OS notification is requested, asked
 * for or sent.
 *
 * A toast is for *news*. Nothing is announced on the first answer, whatever is
 * on the list by then — the badge and the title already carry that, and a
 * window opened onto six stuck agents would otherwise open onto six toasts. Nor
 * is anything announced while the board is up, where the strip is already
 * saying it in a place that does not disappear after six seconds. Both cases
 * still count as seen, so leaving the board does not replay them.
 *
 * All of it comes off the same three shared queries the strip reads (see
 * `attention.ts`), which the SSE dispatcher keeps current — so an agent that
 * gets blocked while the user is on Profiles reaches them without this
 * subscribing to anything or polling for it.
 */

import { useEffect, useRef } from "react"
import { useLocation, useNavigate, useSearchParams } from "react-router-dom"
import { toast } from "sonner"

import { Badge } from "@/components/ui/badge"
import { SESSION_ATTENTION_META } from "@/features/sessions/session-display"
import { STALLED_META, TASK_STATUS_META } from "@/features/tasks"
import { plural } from "@/lib/format"
import { paths } from "@/routes/paths"

import {
  type AttentionItem,
  attentionSubject,
  attentionTarget,
  recoveryHeadline,
  useAttention,
} from "./attention"

/**
 * The title of a window with nothing waiting in it — the one `index.html`
 * carries, kept in step with it by hand.
 */
const QUIET_TITLE = "Ariadne Desktop"

/**
 * The title while the list itself could not be read in full — a failed
 * query, or the recovery producer's own `complete: false` — with zero items
 * to show for it. Distinct from {@link QUIET_TITLE} on purpose: an empty
 * board under a read that did not finish is unknown, not "nothing needs
 * attention" (009).
 */
const UNAVAILABLE_TITLE = "(!) Ariadne"

/**
 * The title while something does. The count leads, and what follows it is the
 * short name: a tab strip gives a title a couple of dozen pixels, and the
 * number is the part that has to survive being cut off.
 */
const counted = (count: number) => `(${count}) Ariadne`

/** Mounted once by the shell; it draws nothing of its own. */
export function AttentionAlerts() {
  const attention = useAttention()
  const count = attention.items.length
  // A failed read with nothing to show for it is unknown, not quiet.
  const unavailable = count === 0 && attention.error !== null
  const onBoard = useLocation().pathname === paths.goals()

  useEffect(() => {
    document.title = count > 0 ? counted(count) : unavailable ? UNAVAILABLE_TITLE : QUIET_TITLE
    return () => {
      document.title = QUIET_TITLE
    }
  }, [count, unavailable])

  useAttentionToasts(attention.items, {
    // A list still loading has no news in it, and a list with a failed query
    // in it cannot tell news from a hole: an item missing because
    // `GET /v1/sessions` failed would be announced all over again on the
    // retry. Both wait, and the strip's error row is what says so.
    ready: !attention.isPending && !attention.error,
    quiet: onBoard,
  })

  return null
}

/** The count on the Goals entry in the sidebar, absent while there is none. */
export function AttentionBadge() {
  const { items, error } = useAttention()
  // A failed read with nothing to show for it still gets a badge: an empty
  // one here would read as "nothing needs attention" (009), which a read
  // that did not finish has no business saying.
  if (items.length === 0 && error === null) return null
  return (
    <Badge
      // The warn step of the status ramp, which is what every attention badge
      // on the board and in the panels is drawn in.
      className="ml-auto bg-status-warn-soft text-status-warn-fg"
      aria-label={
        items.length > 0
          ? `${plural(items.length, "item")} needing attention`
          : "could not read what needs attention"
      }
    >
      {items.length > 0 ? items.length : "!"}
    </Badge>
  )
}

/**
 * One toast per item that has newly become stuck, and none for an item that
 * was already on the list.
 *
 * What counts as "the same item" is the row *and its reasons*: an author
 * that gets blocked on a permission prompt while its task was already failed
 * is news on a row that was already there, and the row folds the two together
 * (see `attention.ts`) — keying on the id alone would swallow it.
 */
function useAttentionToasts(
  items: AttentionItem[],
  { ready, quiet }: { ready: boolean; quiet: boolean },
) {
  const navigate = useNavigate()
  const [search] = useSearchParams()
  // These fire on whichever screen the user is on, and where a row opens
  // depends on that screen: see {@link attentionTarget}.
  const { pathname } = useLocation()
  /** What has been on the list already; `null` until the first full answer. */
  const announced = useRef<Set<string> | null>(null)

  useEffect(() => {
    if (!ready) return
    const current = new Map(items.map((item) => [alertKey(item), item]))
    const seen = announced.current
    announced.current = new Set(current.keys())
    // The first answer is the state of the world, not news about it.
    if (!seen) return

    for (const [key, item] of current) {
      if (seen.has(key) || quiet) continue
      const target = attentionTarget(item, search, pathname)
      toast.warning(headline(item), {
        // One toast per item: a reason raised twice in a row — a stream that
        // dropped and re-delivered, a re-render — replaces its own toast
        // rather than stacking a second one under it.
        id: `attention-${key}`,
        description: attentionSubject(item),
        // `target.replace` when it is a task panel swapping in for an open
        // goal's, same reasoning as the strip's own row.
        action: {
          label: "Open",
          onClick: () => void navigate(target, { replace: target.replace }),
        },
      })
    }
  }, [items, ready, quiet, navigate, search, pathname])
}

/** What has changed about this row, as far as the list is concerned. */
function alertKey(item: AttentionItem): string {
  return `${item.id}:${item.taskReason ?? "-"}:${item.sessionReason ?? "-"}`
}

/** What the toast leads with: the reason, in the words the badges use. */
function headline(item: AttentionItem): string {
  if (item.recovery) return recoveryHeadline(item.recovery)
  if (item.sessionReason) return SESSION_ATTENTION_META[item.sessionReason].label
  if (item.taskReason === "stalled") return STALLED_META.label
  return TASK_STATUS_META.failed.label
}
