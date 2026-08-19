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
 * The rows are links into the panel scheme the rest of the app uses — `?task=`
 * for a task, `?session=` for a session — so reading the list and acting on it
 * are the same gesture, and the board stays underneath.
 *
 * Nothing here polls, and nothing here reads the board's status filter; see
 * `attention.ts`.
 */

import { Link, useSearchParams } from "react-router-dom"

import type { GoalDto } from "@/api"
import { StatusBadge } from "@/components/status-badge"
import { SESSION_ATTENTION_META, SessionAttentionBadge } from "@/features/sessions/session-display"
import { STALLED_META, StalledBadge, TASK_STATUS_META } from "@/features/tasks"
import { describeError } from "@/lib/errors"
import { shortId } from "@/lib/ids"
import { ROLE_LABELS } from "@/lib/labels"
import { plural } from "@/lib/plural"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { sessionPanelTo, taskPanelTo } from "@/routes/paths"

import type { AttentionSessionItem, AttentionTaskItem } from "./attention"
import { useAttention } from "./attention"

export function AttentionStrip() {
  const attention = useAttention()

  // Nothing stuck is the normal state of a healthy board, and the board is
  // what the screen is for: the strip is absent rather than reassuring.
  if (attention.items.length === 0) return null

  return (
    <section aria-label="Needs attention" className="shrink-0 rounded-lg border">
      <header className="flex items-baseline gap-2 border-b px-3 py-2">
        <h2 className="font-heading text-sm font-semibold">Needs attention</h2>
        <span className="text-xs text-muted-foreground">
          {plural(attention.items.length, "item")}
        </span>
      </header>

      {/* Tall enough to read a handful of rows at a glance, capped so a bad
          morning does not push the board off the screen. Two lines a row now,
          so the cap is a little taller than the handful it used to hold. */}
      <ul className="max-h-64 divide-y overflow-y-auto">
        {attention.items.map((item) =>
          item.kind === "task" ? (
            <TaskRow key={item.id} item={item} />
          ) : (
            <SessionRow key={item.id} item={item} />
          ),
        )}
      </ul>

      {/* One of the three queries can fail while the others answered, and what
          is above is a real list with holes in it — not a list that failed to
          load. Saying so is the difference between "read this, some of it is
          missing" and letting a short list pass for the whole truth. */}
      {attention.partial ? (
        <p className="border-t px-3 py-2 text-xs text-muted-foreground">
          Some of this list could not be loaded — {describeError(attention.error)}.{" "}
          <button
            type="button"
            onClick={attention.refetch}
            className="underline underline-offset-3 hover:text-foreground"
          >
            Retry
          </button>
        </p>
      ) : null}
    </section>
  )
}

/** The two row kinds sit under one another, so they carry the same tails. */
const ROW_LINK =
  "flex flex-wrap items-center gap-2 px-3 py-2 text-sm transition-colors hover:bg-muted/50"

function TaskRow({ item: { task, reason, goalId, goal } }: { item: AttentionTaskItem }) {
  const [search] = useSearchParams()
  const meta = TASK_STATUS_META[task.status]

  return (
    <li>
      <Link to={taskPanelTo(search, task.id)} className={ROW_LINK}>
        <StatusBadge box="badge" label={meta.label} tone={meta.badge} hint={meta.hint} />
        {/* The status pill already names a failed task; a stall is a flag on
            top of whatever status the task is sitting in. */}
        {reason === "stalled" ? <StalledBadge /> : null}
        <Subject
          subject={task.title}
          detail={`Task · ${reason === "stalled" ? STALLED_META.hint : meta.hint}`}
        />
        <GoalRef goalId={goalId} goal={goal} />
        <Age at={task.updated_at} />
        <RowId id={task.id} />
      </Link>
    </li>
  )
}

function SessionRow({
  item: { session, reason, goalId, goal, task, at },
}: {
  item: AttentionSessionItem
}) {
  const [search] = useSearchParams()

  return (
    <li>
      {/* A session opens in a panel of its own, over the board — the same one
          for a planner session, which belongs to no task. */}
      <Link to={sessionPanelTo(search, session.id)} className={ROW_LINK}>
        {/* The reason, not the status: a session blocked on a permission
            prompt is still running, and "Running" is not why it is on this
            list. `SessionStatusBadge` stays what the sessions feature shows,
            where a badge has five statuses to tell apart. */}
        <SessionAttentionBadge attention={reason} />
        <Subject
          // What the agent was working on, which is what decides whether this
          // row is worth opening — the goal on the right is only where it
          // sits. A session with no task is a planner's, and is named by the
          // one thing it does have: its role and the goal it is planning.
          subject={
            session.task_id
              ? (task?.title ?? `Task ${shortId(session.task_id)}`)
              : `${ROLE_LABELS[session.role]} · ${goal?.title ?? `Goal ${shortId(goalId)}`}`
          }
          // Who is asking and what for, spelled out: the pill's label is four
          // words, and which agent of the three raised it is the other half of
          // knowing whether this row is yours to answer. A task-less session
          // already says its role in the subject.
          detail={
            session.task_id
              ? `${ROLE_LABELS[session.role]} · ${SESSION_ATTENTION_META[reason].hint}`
              : SESSION_ATTENTION_META[reason].hint
          }
        />
        <GoalRef goalId={goalId} goal={goal} />
        <Age at={at} />
        <RowId id={session.id} />
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
 */
function Subject({ subject, detail }: { subject: string; detail: string }) {
  return (
    <span className="min-w-0 flex-1">
      <span className="block truncate font-medium" title={subject}>
        {subject}
      </span>
      <span className="block truncate text-xs text-muted-foreground" title={detail}>
        {detail}
      </span>
    </span>
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
    <span className="max-w-40 shrink-0 truncate text-xs text-muted-foreground" title={title}>
      {title}
    </span>
  )
}

/** When this row last moved, the one way for both kinds. */
function Age({ at }: { at: string }) {
  return (
    <time dateTime={at} className="text-xs text-muted-foreground" title={formatAbsolute(at)}>
      {formatRelative(at)}
    </time>
  )
}

function RowId({ id }: { id: string }) {
  return (
    <span className="font-mono text-xs text-muted-foreground" title={id}>
      {shortId(id)}
    </span>
  )
}
