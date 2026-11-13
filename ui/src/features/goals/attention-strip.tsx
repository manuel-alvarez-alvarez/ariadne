/** The shell's persistent entry point to the shared live attention list. */

import { useState } from "react"
import { Link, useLocation, useSearchParams } from "react-router-dom"

import type { GoalDto } from "@/api"
import { StatusBadge } from "@/components/status-badge"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When } from "@/components/when"
import { SessionAttentionBadge } from "@/features/sessions/session-display"
import { StalledBadge, TASK_STATUS_META } from "@/features/tasks"
import { describeError, shortId } from "@/lib/format"

import {
  type AttentionItem,
  attentionAffected,
  attentionAffectedLinks,
  attentionDetail,
  attentionSubject,
  attentionTarget,
  useAttention,
} from "./attention"

/** The shared attention list, available from every screen. */
export function AttentionPopover() {
  const attention = useAttention()
  const [open, setOpen] = useState(false)
  const unavailable = attention.items.length === 0 && attention.error !== null
  const empty =
    attention.items.length === 0 && !attention.isPending && !unavailable && !attention.partial
  if (empty) return null
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <button
            type="button"
            className="h-8 rounded-md border px-2 text-sm text-status-warn-fg"
            aria-label={`Needs attention, ${attention.items.length} items`}
          />
        }
      >
        Needs attention{" "}
        <span className="ml-1">{attention.items.length || (attention.isPending ? "…" : "!")}</span>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[min(24rem,calc(100vw-1rem))] p-0">
        <header className="border-b px-3 py-2 font-semibold">
          Needs attention · {attention.items.length}
        </header>
        {attention.isPending && attention.items.length === 0 ? (
          <p role="status" className="px-3 py-2">
            Loading attention…
          </p>
        ) : null}
        {unavailable ? (
          <p className="px-3 py-2">
            Could not load attention — {describeError(attention.error)}.{" "}
            <Retry onClick={attention.refetch} />
          </p>
        ) : null}
        <ul className="max-h-[min(70vh,32rem)] divide-y overflow-y-auto">
          {attention.items.map((item) => (
            <Row key={item.id} item={item} onSelect={() => setOpen(false)} />
          ))}
        </ul>
        {attention.partial ? (
          <p className="border-t px-3 py-2 text-xs text-muted-foreground">
            Some items could not load — {describeError(attention.error)}.{" "}
            <Retry onClick={attention.refetch} />
          </p>
        ) : null}
      </PopoverContent>
    </Popover>
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
 * question opens the thread it was asked in and a prompt opens the console it
 * is waiting in.
 */
function Row({ item, onSelect }: { item: AttentionItem; onSelect?: () => void }) {
  const [search] = useSearchParams()
  // Where a row lands depends on the screen as well as its params: the strip is
  // the board's, but the same targets are used from the alerts, which are
  // everywhere (see {@link attentionTarget}).
  const { pathname } = useLocation()
  const status = item.taskReason && item.task ? TASK_STATUS_META[item.task.status] : null
  const target = attentionTarget(item, search, pathname)
  // A grouped quota item's own target opens only one of its sessions;
  // every one of them gets its own small link below, so none of them is
  // unreachable just because another one happened to be first.
  const affectedLinks = attentionAffectedLinks(item, search, pathname)
  const className =
    "flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-2 text-sm transition-colors hover:bg-muted/50"
  const content = (
    <>
      {status ? (
        <StatusBadge box="badge" label={status.label} tone={status.badge} hint={status.hint} />
      ) : null}
      {/* The status pill already names a failed task; a stall is a flag on
          top of whatever status the task is sitting in. */}
      {item.taskReason === "stalled" ? <StalledBadge /> : null}
      {item.sessionReason ? <SessionAttentionBadge attention={item.sessionReason} /> : null}
      <Subject
        subject={attentionSubject(item)}
        detail={attentionDetail(item)}
        affected={attentionAffected(item)}
      />
      {/* Its own line while the strip is narrow, where the goal, the stamp
          and the id together took half the row and left the subject twenty
          characters. */}
      <div className="flex w-full items-center gap-2 @2xl:w-auto">
        {/* A pull request's session belongs to no goal to name. */}
        {item.goalId ? <GoalRef goalId={item.goalId} goal={item.goal} /> : null}
        {/* One label for every row: a task's row moves when the task is
            updated, a session's when it started asking — "last moved" is the
            one thing true of both, and the list is ordered by it. */}
        <When at={item.at} label="last moved" className="text-xs text-muted-foreground" />
        <RowId id={item.id} />
      </div>
    </>
  )

  return (
    <li>
      <Link
        to={target}
        // Set exactly when the target is a task panel replacing an open
        // goal's, rather than stacking on it (`target.replace`).
        replace={target.replace}
        onClick={onSelect}
        className={className}
      >
        {content}
      </Link>
      {affectedLinks ? (
        <div className="flex flex-wrap gap-2 px-3 pb-2 pl-9 text-xs text-muted-foreground">
          <span>Open:</span>
          {affectedLinks.map((link) => (
            <Link
              key={link.id}
              to={link.to}
              className="underline underline-offset-2 hover:text-foreground"
            >
              {link.label}
            </Link>
          ))}
        </div>
      ) : null}
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
function Subject({
  subject,
  detail,
  affected,
}: {
  subject: string
  detail: string
  /** A recovery row's own `affected` list, joined — null for every other row. */
  affected: string | null
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="min-w-0 flex-1" />}>
        <span className="block truncate font-medium">{subject}</span>
        <span className="block truncate text-xs text-muted-foreground">{detail}</span>
        {affected ? (
          <span className="block truncate text-xs text-muted-foreground">Affects: {affected}</span>
        ) : null}
      </TooltipTrigger>
      <TooltipContent className="flex-col items-start gap-0.5">
        <span className="font-medium">{subject}</span>
        <span className="text-background/70">{detail}</span>
        {affected ? <span className="text-background/70">Affects: {affected}</span> : null}
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
