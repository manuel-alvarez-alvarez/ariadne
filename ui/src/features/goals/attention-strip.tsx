/**
 * Everything that is stuck, in one flat list above the board.
 *
 * The board answers "what is being worked on"; this answers "what is waiting
 * for me", which is the question that gets asked first. It sits between the
 * header and the lanes rather than on a screen of its own, so the answer is
 * already there when the board opens — and it is one list rather than a list
 * per goal: five goals with one stuck task each is five rows, not five
 * headings. Each row names its goal instead, since nothing above it does.
 *
 * One row per thing gone wrong, too: a failed task under an agent that
 * reported an error is one row with two badges rather than the same trouble
 * twice (see `attention.ts`). Every row is a link to the control the answer is
 * given through — the thread a question was asked in, the pane a prompt is
 * waiting in — so reading the list and acting on it are the same gesture, and
 * the board stays underneath.
 *
 * The list never clips: what does not fit is counted and expanded on a click,
 * because a strip that says "8 items" and shows five with no scrollbar is a
 * strip that hides exactly the row nobody knew to look for. Nor does it go
 * quiet when it cannot answer — a failed `GET /v1/sessions` is an error row,
 * not an empty board.
 *
 * Nothing here polls, and nothing here reads the board's status filter; see
 * `attention.ts`. The same count reaches the rest of the app through
 * `attention-alerts.tsx`, for the screens this strip is not on.
 */

import { TriangleAlertIcon } from "lucide-react"
import { type ReactNode, useState } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { GoalDto } from "@/api"
import { StatusBadge } from "@/components/status-badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When } from "@/components/when"
import { SessionAttentionBadge } from "@/features/sessions/session-display"
import { StalledBadge, TASK_STATUS_META } from "@/features/tasks"
import { describeError, plural, shortId } from "@/lib/format"

import {
  type AttentionItem,
  attentionDetail,
  attentionSubject,
  attentionTarget,
  useAttention,
} from "./attention"

/**
 * How many rows are shown before the rest are counted rather than listed.
 *
 * Enough that the usual morning is one glance and no click, small enough that
 * a bad one does not push the board off the screen — and the rest are one
 * click away rather than behind a scrollbar macOS does not draw.
 */
const VISIBLE_ROWS = 5

export function AttentionStrip() {
  const attention = useAttention()
  const [expanded, setExpanded] = useState(false)

  if (attention.items.length === 0) {
    // Nothing loaded and nothing said why: the list is the one thing on this
    // screen that must not read as "all clear" when it is really "unknown".
    if (attention.error) {
      return (
        <Frame>
          <p className="flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-2 text-xs text-muted-foreground">
            <TriangleAlertIcon className="size-3.5 shrink-0 text-status-warn" aria-hidden />
            <span>Could not load what needs attention — {describeError(attention.error)}.</span>
            <Retry onClick={attention.refetch} />
          </p>
        </Frame>
      )
    }
    // A thin bar rather than the strip's own shape: until the three lists
    // answer, how tall the list is going to be is not known, and a tall
    // placeholder that collapses to nothing shifts the board under the cursor.
    if (attention.isPending) {
      return (
        <Skeleton
          role="status"
          aria-label="Loading what needs attention"
          className="h-9 shrink-0"
        />
      )
    }
    // Nothing stuck is the normal state of a healthy board, and the board is
    // what the screen is for: the strip is absent rather than reassuring.
    return null
  }

  const hidden = attention.items.length - VISIBLE_ROWS
  const rows = expanded ? attention.items : attention.items.slice(0, VISIBLE_ROWS)

  return (
    <Frame>
      <header className="flex items-baseline gap-2 border-b px-3 py-2">
        <h2 className="font-heading text-sm font-semibold">Needs attention</h2>
        <span className="text-xs text-muted-foreground">
          {plural(attention.items.length, "item")}
        </span>
      </header>

      {/* A container query rather than a viewport one: how much room a row has
          is the strip's width — the sidebar and the panel padding are between
          it and the window — so at 900px the meta used to take half of a row
          the window still called wide. */}
      <ul className="@container divide-y">
        {rows.map((item) => (
          <Row key={item.id} item={item} />
        ))}
      </ul>

      {hidden > 0 ? (
        <div className="border-t px-3 py-1.5">
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            className="text-xs text-muted-foreground underline-offset-3 hover:text-foreground hover:underline"
          >
            {expanded ? "Show fewer" : `${hidden} more…`}
          </button>
        </div>
      ) : null}

      {/* One of the three queries can fail while the others answered, and what
          is above is a real list with holes in it — not a list that failed to
          load. Saying so is the difference between "read this, some of it is
          missing" and letting a short list pass for the whole truth. */}
      {attention.partial ? (
        <p className="border-t px-3 py-2 text-xs text-muted-foreground">
          Some of this list could not be loaded — {describeError(attention.error)}.{" "}
          <Retry onClick={attention.refetch} />
        </p>
      ) : null}
    </Frame>
  )
}

/** The bordered box, whichever of the three things is inside it. */
function Frame({ children }: { children: ReactNode }) {
  return (
    <section aria-label="Needs attention" className="shrink-0 rounded-lg border">
      {children}
    </section>
  )
}

function Retry({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="underline underline-offset-3 hover:text-foreground"
    >
      Retry
    </button>
  )
}

/**
 * One thing that wants a person: what it is, why, and the way to answer it.
 *
 * The badges are the reasons, task first — a task's status is what the row
 * sits under, and the session's reason is the live thing on top of it. Where
 * the row goes is the reason's to say (see {@link attentionTarget}), so a
 * question opens the thread it was asked in and a prompt opens the pane it is
 * waiting in.
 */
function Row({ item }: { item: AttentionItem }) {
  const [search] = useSearchParams()
  const status = item.taskReason && item.task ? TASK_STATUS_META[item.task.status] : null

  return (
    <li>
      <Link
        to={attentionTarget(item, search)}
        className="flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-2 text-sm transition-colors hover:bg-muted/50"
      >
        {status ? (
          <StatusBadge box="badge" label={status.label} tone={status.badge} hint={status.hint} />
        ) : null}
        {/* The status pill already names a failed task; a stall is a flag on
            top of whatever status the task is sitting in. */}
        {item.taskReason === "stalled" ? <StalledBadge /> : null}
        {item.sessionReason ? <SessionAttentionBadge attention={item.sessionReason} /> : null}
        <Subject subject={attentionSubject(item)} detail={attentionDetail(item)} />
        {/* Its own line while the strip is narrow, where the goal, the stamp
            and the id together took half the row and left the subject twenty
            characters. */}
        <div className="flex w-full items-center gap-2 @2xl:w-auto">
          <GoalRef goalId={item.goalId} goal={item.goal} />
          {/* One label for every row: a task's row moves when the task is
              updated, a session's when it started asking — "last moved" is the
              one thing true of both, and the list is ordered by it. */}
          <When at={item.at} label="last moved" className="text-xs text-muted-foreground" />
          <RowId id={item.id} />
        </div>
      </Link>
    </li>
  )
}

/**
 * What the row is about, over what it is: the title on top in the row's own
 * size, the explanation under it in the muted one.
 *
 * Two lines rather than one, because the two used to compete for the same
 * truncating span — and it was the title, the only part that identifies the
 * row, that lost first. They now truncate independently, so a narrow strip
 * shortens both instead of dropping one.
 *
 * Both still truncate, so what a row says in full is only ever in the hint —
 * one `Tooltip` over the pair rather than a `title=` on each, which is what
 * puts it in reach of a keyboard.
 */
function Subject({ subject, detail }: { subject: string; detail: string }) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="min-w-0 flex-1" />}>
        <span className="block truncate font-medium">{subject}</span>
        <span className="block truncate text-xs text-muted-foreground">{detail}</span>
      </TooltipTrigger>
      <TooltipContent className="flex-col items-start gap-0.5">
        <span className="font-medium">{subject}</span>
        <span className="text-background/70">{detail}</span>
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * Which goal this row belongs to — the one thing the old grouped screen said
 * that a flat list otherwise loses.
 *
 * Plain text rather than a link to the goal: the row is already a link, and
 * the panel it opens carries the goal anyway.
 */
function GoalRef({ goalId, goal }: { goalId: string; goal: GoalDto | undefined }) {
  const title = goal?.title ?? `Goal ${shortId(goalId)}`
  return (
    <Tooltip>
      <TooltipTrigger
        render={<span className="max-w-40 shrink-0 truncate text-xs text-muted-foreground" />}
      >
        {title}
      </TooltipTrigger>
      <TooltipContent>{title}</TooltipContent>
    </Tooltip>
  )
}

function RowId({ id }: { id: string }) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="font-mono text-xs text-muted-foreground" />}>
        {shortId(id)}
      </TooltipTrigger>
      <TooltipContent className="font-mono">{id}</TooltipContent>
    </Tooltip>
  )
}
