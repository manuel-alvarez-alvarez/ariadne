/** The shell's persistent entry point to the shared live attention list. */

import { useRef, useState } from "react"

import { HostLabel } from "@/components/host-label"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { SESSION_ATTENTION_META } from "@/features/sessions/session-display"
import { STALLED_META, TASK_STATUS_META } from "@/features/tasks"
import { describeError, shortId } from "@/lib/format"

import { type AttentionItem, recoveryHeadline, useAttention } from "./attention"
import { AttentionBlock } from "./attention-block"

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
          {unavailable ? <HostErrors attention={attention} className="px-3 py-2" /> : null}
          <ul className="max-h-[min(70vh,32rem)] divide-y overflow-y-auto">
            {attention.items.map((item) => (
              <AttentionBlock key={rowKey(item)} item={item} onSelect={() => setOpen(false)} />
            ))}
          </ul>
          {attention.partial ? (
            <HostErrors
              attention={attention}
              className="border-t px-3 py-2 text-xs text-muted-foreground"
              partial
            />
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
      {unavailable ? <HostErrors attention={attention} /> : null}
      {!attention.isPending && !unavailable && attention.items.length === 0 ? (
        <p>Nothing needs attention.</p>
      ) : null}
      {attention.items.length > 0 && shown.length === 0 ? (
        <p>No attention items match these filters.</p>
      ) : null}
      <ul className="divide-y rounded-md border">
        {shown.map((item) => (
          <AttentionBlock key={rowKey(item)} item={item} onSelect={onSelect} expanded />
        ))}
      </ul>
      {attention.partial ? (
        <HostErrors attention={attention} className="text-xs text-muted-foreground" partial />
      ) : null}
    </>
  )
}

/**
 * One line per host whose attention read failed, naming it and giving its
 * reason, each with its own Retry. The other hosts' items stay on the list.
 */
function HostErrors({
  attention,
  className,
  partial = false,
}: {
  attention: ReturnType<typeof useAttention>
  className?: string
  partial?: boolean
}) {
  return attention.errors.map(({ host, error }) => (
    <p key={host} className={className}>
      {partial ? "Some items could not load" : "Could not load attention"} on{" "}
      <HostLabel host={host} /> — {describeError(error)}. <Retry onClick={attention.refetch} />
    </p>
  ))
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

/** A row's React key: ids are only unique on the host that raised them. */
function rowKey(item: AttentionItem): string {
  return `${item.host}\n${item.id}`
}
