/** The shell's persistent entry point to the shared live attention list. */

import { useRef, useState } from "react"
import { Link, useLocation, useSearchParams } from "react-router-dom"

import type { GoalDto } from "@/api"
import { StatusBadge } from "@/components/status-badge"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When } from "@/components/when"
import { SESSION_ATTENTION_META, SessionAttentionBadge } from "@/features/sessions/session-display"
import { STALLED_META, StalledBadge, TASK_STATUS_META } from "@/features/tasks"
import { describeError, shortId } from "@/lib/format"

import {
  type AttentionItem,
  attentionAffected,
  attentionAffectedLinks,
  attentionDetail,
  attentionSubject,
  attentionTarget,
  recoveryHeadline,
  useAttention,
} from "./attention"

/** The shared attention list, available from every screen. */
export function AttentionPopover() {
  const attention = useAttention()
  const [open, setOpen] = useState(false)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const picked = useRef(false)
  const unavailable = attention.items.length === 0 && attention.error !== null
  const empty =
    attention.items.length === 0 && !attention.isPending && !unavailable && !attention.partial
  if (empty && !drawerOpen) return null
  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          render={
            <button
              type="button"
              ref={triggerRef}
              className="h-8 rounded-md border px-2 text-sm text-status-warn-fg"
              aria-label={`Needs attention, ${attention.items.length} items`}
            />
          }
        >
          Needs attention{" "}
          <span className="ml-1">
            {attention.items.length || (attention.isPending ? "…" : "!")}
          </span>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-[min(24rem,calc(100vw-1rem))] p-0">
          <header className="flex items-center justify-between border-b px-3 py-2 font-semibold">
            <span>Needs attention · {attention.items.length}</span>
            <button
              type="button"
              onClick={() => {
                setOpen(false)
                setDrawerOpen(true)
              }}
              className="text-xs font-normal underline underline-offset-3 hover:text-foreground"
            >
              View all
            </button>
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
      <Sheet
        open={drawerOpen}
        onOpenChange={setDrawerOpen}
        onOpenChangeComplete={(next) => {
          // A picked row moved the keyboard to its target; only a dismissal returns it here.
          if (!next && !picked.current) triggerRef.current?.focus()
          picked.current = false
        }}
      >
        <SheetContent className="sm:max-w-3xl">
          <AttentionDrawer
            attention={attention}
            onSelect={() => {
              picked.current = true
              setDrawerOpen(false)
            }}
          />
        </SheetContent>
      </Sheet>
    </>
  )
}

const NO_GOAL = "__no_goal__"

/** The reasons a row is on the list, as filter keys with their labels. */
function reasonsOf(item: AttentionItem): { key: string; label: string }[] {
  const out: { key: string; label: string }[] = []
  if (item.taskReason === "stalled") out.push({ key: "task:stalled", label: STALLED_META.label })
  else if (item.taskReason && item.task)
    out.push({ key: "task:failed", label: TASK_STATUS_META[item.task.status].label })
  else if (item.taskReason) out.push({ key: "task:failed", label: "Failed" })
  if (item.sessionReason)
    out.push({
      key: `session:${item.sessionReason}`,
      label: SESSION_ATTENTION_META[item.sessionReason].label,
    })
  if (item.recovery)
    out.push({ key: `recovery:${item.recovery.reason}`, label: recoveryHeadline(item.recovery) })
  return out
}

/** Names where {@link attentionTarget} will go, so the label never misstates it. */
function actionLabel(item: AttentionItem): string {
  if (item.recovery) {
    switch (item.recovery.target.kind) {
      case "console":
        return "Open console"
      case "task":
        return "Open task"
      case "pull_request":
        return "Open pull request"
      case "settings":
        return "Open repositories"
    }
  }
  if (
    item.session &&
    (item.sessionReason === "waiting_permission" || item.sessionReason === "waiting_input")
  )
    return "Open console"
  if (!item.taskReason && item.session) return "Open session"
  return "Open task"
}

/** The popover's own list, wider, with a goal and a reason filter over it. */
function AttentionDrawer({
  attention,
  onSelect,
}: {
  attention: ReturnType<typeof useAttention>
  onSelect: () => void
}) {
  const [goal, setGoal] = useState("")
  const [reason, setReason] = useState("")
  const unavailable = attention.items.length === 0 && attention.error !== null
  const goals = new Map<string, string>()
  let hasNoGoal = false
  const reasons = new Map<string, string>()
  for (const item of attention.items) {
    if (item.goalId) goals.set(item.goalId, item.goal?.title ?? `Goal ${shortId(item.goalId)}`)
    else hasNoGoal = true
    for (const r of reasonsOf(item)) reasons.set(r.key, r.label)
  }
  // A value whose option is gone (its last item resolved) is no filter, as the select shows.
  const activeGoal = goal === NO_GOAL ? (hasNoGoal ? goal : "") : goals.has(goal) ? goal : ""
  const activeReason = reasons.has(reason) ? reason : ""
  const shown = attention.items.filter(
    (item) =>
      (!activeGoal || (activeGoal === NO_GOAL ? !item.goalId : item.goalId === activeGoal)) &&
      (!activeReason || reasonsOf(item).some((r) => r.key === activeReason)),
  )
  const selectClass = "h-8 rounded-md border bg-background px-2 text-sm"
  return (
    <>
      <SheetHeader>
        <SheetTitle>Needs attention</SheetTitle>
        <SheetDescription>
          Showing {shown.length} of {attention.items.length}
        </SheetDescription>
      </SheetHeader>
      <div className="flex flex-wrap gap-2">
        <label className="flex items-center gap-1 text-xs">
          Goal
          <select
            className={selectClass}
            value={activeGoal}
            onChange={(event) => setGoal(event.target.value)}
          >
            <option value="">All goals</option>
            {[...goals].map(([id, title]) => (
              <option key={id} value={id}>
                {title}
              </option>
            ))}
            {hasNoGoal ? <option value={NO_GOAL}>No goal</option> : null}
          </select>
        </label>
        <label className="flex items-center gap-1 text-xs">
          Reason
          <select
            className={selectClass}
            value={activeReason}
            onChange={(event) => setReason(event.target.value)}
          >
            <option value="">All reasons</option>
            {[...reasons].map(([key, label]) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {attention.isPending && attention.items.length === 0 ? (
        <p role="status">Loading attention…</p>
      ) : null}
      {unavailable ? (
        <p>
          Could not load attention — {describeError(attention.error)}.{" "}
          <Retry onClick={attention.refetch} />
        </p>
      ) : null}
      {!attention.isPending && !unavailable && attention.items.length === 0 ? (
        <p>Nothing needs attention.</p>
      ) : null}
      {attention.items.length > 0 && shown.length === 0 ? (
        <p>No attention items match these filters.</p>
      ) : null}
      <ul className="divide-y rounded-md border">
        {shown.map((item) => (
          <Row key={item.id} item={item} onSelect={onSelect} expanded />
        ))}
      </ul>
      {attention.partial ? (
        <p className="text-xs text-muted-foreground">
          Some items could not load — {describeError(attention.error)}.{" "}
          <Retry onClick={attention.refetch} />
        </p>
      ) : null}
    </>
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
function Row({
  item,
  onSelect,
  expanded = false,
}: {
  item: AttentionItem
  onSelect?: () => void
  /** The drawer's row: the whole text, and the action named. */
  expanded?: boolean
}) {
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
      {expanded ? (
        <span className="min-w-0 flex-1">
          <span className="block font-medium break-words">{attentionSubject(item)}</span>
          <span className="block text-xs break-words text-muted-foreground">
            {attentionDetail(item)}
          </span>
          {attentionAffected(item) ? (
            <span className="block text-xs break-words text-muted-foreground">
              Affects: {attentionAffected(item)}
            </span>
          ) : null}
        </span>
      ) : (
        <Subject
          subject={attentionSubject(item)}
          detail={attentionDetail(item)}
          affected={attentionAffected(item)}
        />
      )}
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
        {expanded ? (
          <span className="text-xs font-medium underline">{actionLabel(item)}</span>
        ) : null}
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
