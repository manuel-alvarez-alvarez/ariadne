/** A persistent queue beside the current intervention. Panel parameters remain separate. */

import { useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { useHostLists } from "@/api"
import { HostDot } from "@/components/host-dot"
import { HostLabel } from "@/components/host-label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { SessionConsole } from "@/features/sessions/console/session-console"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { taskListQueryOptions } from "@/features/tasks"
import { useReachableHosts } from "@/hooks/use-host-filter"
import { attentionDetail, attentionSubject, useAttention } from "./attention"
import { HostErrors, Row, reasonsOf, rowKey } from "./attention-strip"
import { goalsQueryOptions } from "./queries"

/** The select value of "All"; no `choice` value equals it. */
const ALL = "all"
/** Every other option's select value: the id behind a prefix, so no id is "All". */
const choice = (id: string) => `id:${id}`

export function AttentionPage() {
  const [queueVisible, setQueueVisible] = useState(false)
  const attention = useAttention()
  const reachable = useReachableHosts()
  const sessions = useHostLists(reachable, (h) => sessionsQueryOptions(h))
  const tasks = useHostLists(reachable, (h) => taskListQueryOptions(h))
  const goalsData = useHostLists(reachable, (h) => goalsQueryOptions(h))
  const hydrated = attention.items.map((item) => {
    const target = item.recovery?.target
    const session = sessions.rows.find(
      (row) => row.host === item.host && target?.kind === "console" && row.id === target.session_id,
    )
    const taskId = target?.kind === "task" ? target.task_id : session?.task_id
    const task = tasks.rows.find((row) => row.host === item.host && row.id === taskId)
    const goalId =
      target?.kind === "goal" ? target.goal_id : task?.goal_id || session?.goal_id || ""
    return {
      ...item,
      session,
      task,
      goalId,
      goal: goalsData.rows.find((row) => row.host === item.host && row.id === goalId),
    }
  })
  const [search, setSearch] = useSearchParams()
  const order = useRef<string[]>([])
  const keys = attention.items.map(rowKey)
  // Retain positions, including withdrawn rows, for the lifetime of this workspace.
  for (const key of keys) if (!order.current.includes(key)) order.current.push(key)
  const items = [...hydrated].sort(
    (a, b) => order.current.indexOf(rowKey(a)) - order.current.indexOf(rowKey(b)),
  )
  const goal = search.get("attentionGoal") ?? ""
  const reason = search.get("attentionReason") ?? ""
  const host = search.get("attentionHost") ?? ""
  const selected = search.get("attentionItem") ?? ""
  const shown = items.filter(
    (item) =>
      (!goal || (goal === "__no_goal__" ? !item.goalId : item.goalId === goal)) &&
      (!host || item.host === host) &&
      (!reason || reasonsOf(item).some((r) => r.key === reason)),
  )
  const previous = shown[indexOfSelected(shown, selected) - 1]
  const following = shown[indexOfSelected(shown, selected) + 1]
  const current = items.find((item) => rowKey(item) === selected)
  const index = shown.findIndex((item) => rowKey(item) === selected)
  function change(key: string, value: string) {
    if (key === "attentionItem") setQueueVisible(false)
    const next = new URLSearchParams(search)
    if (value) next.set(key, value)
    else next.delete(key)
    setSearch(next)
  }
  const goals = new Map(
    items.map((item) => [
      item.goalId || "__no_goal__",
      item.goal?.title ?? (item.goalId || "No goal"),
    ]),
  )
  const reasons = new Map(
    items.flatMap((item) => reasonsOf(item).map((r) => [r.key, r.label] as const)),
  )
  const hosts = new Map(items.map((item) => [item.host, item.host]))
  function filter(
    label: string,
    key: string,
    value: string,
    options: Map<string, string>,
    isHost = false,
  ) {
    const choices = new Map(options)
    if (value && !choices.has(value)) choices.set(value, value)
    const body = (id: string | null, text: string) => (
      <span className="flex items-center gap-2">
        {isHost && id !== null ? <HostDot host={id} /> : null}
        {text}
      </span>
    )
    const items = [
      { value: ALL, label: body(null, `All ${label.toLowerCase()}s`) },
      ...[...choices].map(([id, text]) => ({ value: choice(id), label: body(id, text) })),
    ]
    return (
      <div className="flex gap-2 items-center">
        {label}
        <Select
          value={value ? choice(value) : ALL}
          items={items}
          onValueChange={(next) =>
            change(key, next === null || next === ALL ? "" : next.slice(choice("").length))
          }
        >
          <SelectTrigger aria-label={label} className="min-w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {items.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    )
  }
  return (
    <div className="flex h-full min-h-0 flex-col gap-3 p-4">
      <div className="flex flex-wrap gap-3">
        {filter("Goal", "attentionGoal", goal, goals)}
        {filter("Reason", "attentionReason", reason, reasons)}
        {filter("Host", "attentionHost", host, hosts, true)}
      </div>
      <HostErrors attention={attention} partial={attention.partial} />
      <p role="status" aria-live="polite">
        {attention.isPending ? "Loading attention…" : `${shown.length} items`}
        {current ? ` · Selected: ${attentionSubject(current)}` : ""}
        {selected && !current && !attention.isPending
          ? " · Selected item resolved or is unavailable. Choose another item."
          : ""}
      </p>
      <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[minmax(16rem,1fr)_2fr]">
        <section
          aria-label="Attention queue"
          className={`${selected && !queueVisible ? "hidden lg:block" : ""} overflow-auto`}
        >
          {!shown.length && !attention.isPending ? (
            <p>No attention items match these filters.</p>
          ) : null}
          <ul className="divide-y rounded border">
            {shown.map((item) => (
              <li key={rowKey(item)}>
                <button
                  type="button"
                  aria-pressed={selected === rowKey(item)}
                  className="w-full p-3 text-left hover:bg-muted focus-visible:ring-2"
                  onClick={() => change("attentionItem", rowKey(item))}
                >
                  <span className="block font-medium">{attentionSubject(item)}</span>
                  <span className="block text-sm">{attentionDetail(item)}</span>
                  <span className="block text-xs">
                    {item.goal?.title} · <HostLabel host={item.host} /> · {item.at}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>
        <section
          aria-label="Selected attention item"
          className={`${!selected || queueVisible ? "hidden lg:flex" : "flex"} min-h-0 flex-col gap-3`}
        >
          <div className="flex gap-3">
            <button
              type="button"
              className="underline lg:hidden"
              onClick={() => setQueueVisible(true)}
            >
              Return to queue
            </button>
            <button
              type="button"
              disabled={index <= 0}
              onClick={() => change("attentionItem", previous ? rowKey(previous) : "")}
            >
              Previous
            </button>
            <button
              type="button"
              disabled={index < 0 || index >= shown.length - 1}
              onClick={() => change("attentionItem", following ? rowKey(following) : "")}
            >
              Next
            </button>
          </div>
          {current ? (
            <>
              <ul className="rounded border">
                <Row item={current} expanded />
              </ul>
              {current.session ? (
                <SessionConsole
                  key={`${current.host}:${current.session.id}`}
                  host={current.host}
                  sessionId={current.session.id}
                  status={current.session.status}
                  session={current.session}
                  className="min-h-0 flex-1"
                />
              ) : null}
            </>
          ) : (
            <p>
              {selected
                ? "Selected item resolved or is unavailable."
                : "Select an item to see its context."}
            </p>
          )}
        </section>
      </div>
    </div>
  )
}

function indexOfSelected(items: import("./attention").AttentionItem[], key: string) {
  return items.findIndex((item) => rowKey(item) === key)
}
