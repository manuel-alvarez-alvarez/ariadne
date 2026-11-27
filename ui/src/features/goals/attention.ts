/**
 * The shared daemon list supplies the attention rows, counts and board badges.
 * Goal, task and session queries keep their loading and retry state, but raw
 * statuses cannot restore an item that the daemon omitted. The SSE dispatcher
 * keeps these shared queries current. The board's filters narrow its groups,
 * never this list.
 */

import { queryOptions, useQueries } from "@tanstack/react-query"
import { useCallback, useMemo } from "react"

import {
  type AttentionCause,
  type AttentionItemDto,
  type AttentionListDto,
  api,
  type GoalDto,
  type Host,
  LOCAL_HOST,
  qk,
  type SessionDto,
  type TaskDto,
  unwrap,
  useHostLists,
} from "@/api"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import {
  SESSION_ATTENTION_META,
  type SessionAttention,
  seatLabel,
  sessionAttention,
} from "@/features/sessions/session-display"
import { STALLED_META, TASK_STATUS_META, taskListQueryOptions } from "@/features/tasks"
import { sessionColumn } from "@/features/tasks/steps"
import { useReachableHosts } from "@/hooks/use-host-filter"
import { shortId } from "@/lib/format"
import { paths, sessionPanelFrom, sessionTerminalFrom, taskPanelFrom } from "@/routes/paths"
import { useSettingsStore } from "@/stores/settings"

import { goalsQueryOptions } from "./queries"

/** `GET /v1/attention` of one host: that daemon's own authoritative recovery list. */
function attentionListQueryOptions(host: Host) {
  return queryOptions({
    queryKey: qk.attention.list(host),
    queryFn: () => unwrap(api(host).GET("/v1/attention")),
  })
}

/** Each host's attention list, in the order of `hosts`. */
function useAttentionLists(hosts: readonly Host[]) {
  return useQueries({
    queries: hosts.map((host) => attentionListQueryOptions(host)),
  })
}

/** Why a task is on the list, strongest first. */
export type AttentionReason = "failed" | "stalled"

/**
 * The board ordering's fallback for a task's bare status: `failed` only.
 * This does not create an attention row. `stalled` is never reported here:
 * it is a flag raised while automatic recovery (a nudge, then a relaunch)
 * is still working the agent, and reporting it immediately would be
 * exactly the "automatic recovery still trying" row 009's attention
 * contract asks never to raise. If the relaunch also fails, the task
 * fails, and `failed` covers it the same as any other failure.
 */
export function taskAttentionReason(task: TaskDto): AttentionReason | null {
  return task.status === "failed" ? "failed" : null
}

/**
 * One row of the list: everything that wants a person about one task, or about
 * one session that belongs to no task.
 *
 * A task and the session running it can both be stuck at once — a `failed`
 * task under an agent that reported an error, a stalled task under a stalled
 * session — and they are one thing gone wrong, not two. So the row is the
 * task, and the session it carries is a second badge on it rather than a
 * second row saying the same thing a line apart. An orchestrator's session, which
 * belongs to no task, is a row in its own right.
 */
export interface AttentionItem {
  /** The host whose daemon raised the row; every link and answer goes there. */
  host: Host
  /**
   * What identifies the row on its host: the task it is about, or the
   * task-less session that is one. With the host, its React key, and what the
   * toast that announced it remembers.
   */
  id: string
  goalId: string
  /** The goal itself, when the goals list has it — the row names it either way. */
  goal: GoalDto | undefined
  /** When this row last moved; the list is ordered by it, newest first. */
  at: string
  /**
   * The task this row is about, when there is one — even when the task list
   * did not carry it, which is why the id is kept apart from the row itself.
   */
  taskId: string | null
  task: TaskDto | undefined
  /** Why the task itself is here; null when only its session is. */
  taskReason: AttentionReason | null
  /**
   * The flagged session folded onto this row, and why the daemon flagged it.
   * The two are set together or not at all.
   */
  session: SessionDto | undefined
  sessionReason: SessionAttention | null
  /**
   * A recovery item with no goal or session of its own — a machine resource
   * or a daemon configuration blocker — read straight off `GET /v1/attention`
   * rather than derived here. Set only on a row no task or session fields
   * above describe; `attentionSubject`/`attentionDetail`/`attentionTarget`
   * check it first.
   */
  recovery?: AttentionItemDto
}

/** What one host's attention read could not do, and why. */
interface AttentionHostError {
  host: Host
  error: unknown
}

interface Attention {
  /** One row per stuck task or task-less session, most recently moved first. */
  items: AttentionItem[]
  isPending: boolean
  /** The first host error, if any: what says the list is not whole. */
  error: unknown
  /** One entry per host whose read failed or was incomplete; the other hosts' items stay. */
  errors: AttentionHostError[]
  /**
   * The hosts whose list is a full answer: it came, and none of its queries
   * failed. Only they can tell news from a hole.
   */
  readyHosts: Host[]
  /**
   * Something failed, but the queries that answered still produced items.
   *
   * The three lists are independent — a failed `GET /v1/tasks` says nothing
   * about the sessions — so the usual case for a list this size is one that
   * rendered with a hole in it rather than one that failed. Reporting both as
   * the same "could not load" is what made a readable list look empty.
   */
  partial: boolean
  refetch: () => void
}

/**
 * A synthetic error for the one case that is not a query failure: the
 * recovery producer answered 200 but said `complete: false`. Folded into
 * the same `error`/`partial` path every other failure already takes, so an
 * empty board under an incomplete read is never read as "nothing needs
 * attention" (009) without a second code path to keep in step with the
 * first.
 */
const INCOMPLETE_RECOVERY_READ = new Error(
  "the recovery producer could not read all of its evidence",
)

/** The attention list over every reachable host, whatever the header's pick. */
export function useAttention(): Attention {
  const hosts = useReachableHosts()
  const goals = useHostLists(hosts, (host) => goalsQueryOptions(host))
  const tasks = useHostLists(hosts, (host) => taskListQueryOptions(host))
  // Keep the shared queries' loading and retry state. Their raw statuses
  // cannot restore rows that the attention endpoint omitted.
  const sessions = useHostLists(hosts, (host) => sessionsQueryOptions(host))
  const recovery = useAttentionLists(hosts)

  const lists = recovery.map((result) => result.data)
  // The hosts come and go, so the memo cannot spread one dependency per list:
  // a read is told apart by the host it is for and when it last landed.
  const stamp = recovery.map((result) => result.dataUpdatedAt).join()
  // biome-ignore lint/correctness/useExhaustiveDependencies: `stamp` stands for `lists`
  const items = useMemo(() => collectAttention(hosts, lists), [hosts, stamp])

  const errors: AttentionHostError[] = []
  const readyHosts: Host[] = []
  hosts.forEach((host, index) => {
    const failed =
      goals.errors.find((entry) => entry.host === host) ??
      tasks.errors.find((entry) => entry.host === host) ??
      sessions.errors.find((entry) => entry.host === host)
    const error =
      failed?.error ??
      recovery[index]?.error ??
      (lists[index]?.complete === false ? INCOMPLETE_RECOVERY_READ : null)
    if (error) errors.push({ host, error })
    // A list read again is not an answer yet: a host that came back holds its
    // cache from before it left, and only the fresh read tells what it raised since.
    else if (lists[index] && !recovery[index]?.isFetching) readyHosts.push(host)
  })

  return {
    items,
    isPending:
      goals.isPending ||
      tasks.isPending ||
      sessions.isPending ||
      recovery.length === 0 ||
      recovery.every((result) => result.isPending),
    error: errors[0]?.error ?? null,
    errors,
    readyHosts,
    partial: errors.length > 0 && items.length > 0,
    refetch: () => {
      goals.refetch()
      tasks.refetch()
      sessions.refetch()
      for (const result of recovery) if (result.error) void result.refetch()
    },
  }
}

/**
 * Only the shared lists supply rows, including during incomplete reads.
 * Raw task statuses and session flags cannot say whether an occurrence was
 * dismissed or whether its action still targets live work. Each host's rows
 * keep their host, and the hosts' rows are merged newest first.
 */
function collectAttention(
  hosts: readonly Host[],
  lists: readonly (AttentionListDto | undefined)[],
): AttentionItem[] {
  return hosts
    .flatMap((host, index) => (lists[index]?.items ?? []).map((item) => ({ host, item })))
    .map(({ host, item }) => ({
      host,
      id: item.id,
      goalId: "",
      goal: undefined,
      at: item.since,
      taskId: null,
      task: undefined,
      taskReason: null,
      session: undefined,
      sessionReason: null,
      recovery: item,
    }))
    .sort(
      (a, b) =>
        b.at.localeCompare(a.at) || a.id.localeCompare(b.id) || a.host.localeCompare(b.host),
    )
}

/**
 * When a flagged session's row last moved.
 *
 * When the reason was raised is what the row is about, so it comes first: a
 * session that has been waiting on a permission prompt for an hour says so,
 * rather than reporting when it started. A failed session's last move is its
 * death; `created_at` is only the fallback for one the daemon has not stamped
 * an end on yet. `session_at` in `attention.rs` ages the CLI's rows by the
 * same three.
 */
function sessionAttentionAt(session: SessionDto): string {
  return session.attention_since ?? session.ended_at ?? session.created_at ?? ""
}

/**
 * Which cards on the board are asking for a person, indexed by what the agent
 * was working on.
 *
 * The strip answers "what is waiting for me"; this answers the same question
 * where the work itself is, so a blocked task is recognisable in its row
 * without reading the list above it. It is the sessions query the strip
 * already holds — same shared key, so the board costs no extra request and
 * goes live off the same SSE invalidation — narrowed to the one thing a card
 * can show.
 */
export interface BoardAttention {
  /** Task id → why one of its sessions wants a person. */
  byTask: Map<string, SessionAttention>
  /**
   * Goal id → why its orchestrator wants a person. Kept apart from `byTask`
   * because an orchestrator session belongs to no task and so has no card of its
   * own; its goal line is the only place it can be seen.
   */
  byGoal: Map<string, SessionAttention>
  /** Task id → the latest `last_activity_at` of any of its sessions, previous ones included. */
  lastActivity: Map<string, string>
  /**
   * Task id → every shared-list item that names the task, or one of its
   * sessions, newest first: the blocks drawn below its row.
   */
  blocksByTask: Map<string, AttentionItem[]>
  /**
   * Goal id → every item that names the goal, or a session of it that has no
   * task, newest first: the blocks drawn below its goal line.
   */
  blocksByGoal: Map<string, AttentionItem[]>
}

/** Nothing on any card: what a host with no sessions read yet shows. */
export const NO_BOARD_ATTENTION: BoardAttention = {
  byTask: new Map(),
  byGoal: new Map(),
  lastActivity: new Map(),
  blocksByTask: new Map(),
  blocksByGoal: new Map(),
}

/**
 * Each host's board badges, by host: task and goal ids are only unique on
 * their own daemon, so every host is indexed apart.
 */
export function useBoardAttention(hosts: readonly Host[]): Map<Host, BoardAttention> {
  const sessionResults = useQueries({
    queries: hosts.map((host) => sessionsQueryOptions(host)),
  })
  const listResults = useAttentionLists(hosts)
  const sessions = sessionResults.map((result) => result.data)
  const lists = listResults.map((result) => result.data)
  // The hosts come and go, so the memo keys on when each read last landed.
  const stamp = [...sessionResults, ...listResults].map((result) => result.dataUpdatedAt).join()
  // biome-ignore lint/correctness/useExhaustiveDependencies: `stamp` stands for the reads
  return useMemo(
    () =>
      new Map(
        hosts.map((host, index) => [
          host,
          collectBoardAttention(host, sessions[index], lists[index]),
        ]),
      ),
    [hosts, stamp],
  )
}

/**
 * The flagged sessions folded onto their subjects.
 *
 * A task can have several sessions flagged at once — an agent waiting on a
 * permission prompt while a previous column's agent sits disconnected — and a
 * card has room for one badge, so the most recently raised reason wins: it is
 * the one the strip lists first, and the one the user has not seen yet.
 */
function collectBoardAttention(
  host: Host,
  sessions: SessionDto[] | undefined,
  attention?: AttentionListDto,
): BoardAttention {
  const byTask = new Map<string, Flagged>()
  const byGoal = new Map<string, Flagged>()
  const lastActivity = new Map<string, string>()

  for (const session of sessions ?? []) {
    const active = session.last_activity_at
    if (session.task_id && active) {
      const seen = lastActivity.get(session.task_id)
      if (!seen || latest(seen, active) === active) lastActivity.set(session.task_id, active)
    }
    if (
      !attention?.items.some((item) =>
        item.affected.some((subject) => subject.kind === "session" && subject.id === session.id),
      )
    )
      continue
    const reason = sessionAttention(session)
    if (!reason) continue
    const at = sessionAttentionAt(session)
    // A session with no task is an orchestrator's, and lands on its goal's bar.
    const index = session.task_id ? byTask : byGoal
    const key = session.task_id ?? session.goal_id ?? ""
    // A pull request's session has no row and no goal line.
    if (!key) continue
    const held = index.get(key)
    if (!held || held.at.localeCompare(at) < 0) index.set(key, { reason, at })
  }

  return {
    byTask: reasons(byTask),
    byGoal: reasons(byGoal),
    lastActivity,
    ...collectBlocks(host, sessions ?? [], attention),
  }
}

/**
 * The shared list's items, indexed by the task row or the goal line each one
 * is drawn below. An item names a task through an `affected` entry of kind
 * `task`, or of kind `session` through the sessions list: a session with a
 * task is that task's, one without is its goal's. An item that names the same
 * task twice is one block there, and one that names many tasks is a block
 * below each. The list is the only source, also while its read is incomplete.
 */
function collectBlocks(
  host: Host,
  sessions: readonly SessionDto[],
  attention: AttentionListDto | undefined,
): Pick<BoardAttention, "blocksByTask" | "blocksByGoal"> {
  const blocksByTask = new Map<string, AttentionItem[]>()
  const blocksByGoal = new Map<string, AttentionItem[]>()
  const bySession = new Map(sessions.map((session) => [session.id, session]))
  for (const item of collectAttention([host], [attention])) {
    const tasks = new Set<string>()
    const goals = new Set<string>()
    for (const subject of item.recovery?.affected ?? []) {
      if (subject.kind === "task") tasks.add(subject.id)
      else if (subject.kind === "goal") goals.add(subject.id)
      else if (subject.kind === "session") {
        const session = bySession.get(subject.id)
        if (session?.task_id) tasks.add(session.task_id)
        else if (session?.goal_id) goals.add(session.goal_id)
      }
    }
    for (const [index, ids] of [
      [blocksByTask, tasks],
      [blocksByGoal, goals],
    ] as const)
      for (const id of ids) index.set(id, [...(index.get(id) ?? []), item])
  }
  return { blocksByTask, blocksByGoal }
}

/** The later of two RFC 3339 stamps, compared as instants. */
function latest(a: string, b: string): string {
  return Date.parse(b) > Date.parse(a) ? b : a
}

/** One flagged session, while the newest of them is still being picked. */
interface Flagged {
  reason: SessionAttention
  at: string
}

function reasons(index: Map<string, Flagged>): Map<string, SessionAttention> {
  return new Map([...index].map(([key, flagged]) => [key, flagged.reason]))
}

/**
 * Where a row sends the user: not at what is stuck, but at the control the
 * answer is given through.
 *
 * The daemon's reason is what decides, because it is what says *how* the
 * agent is stuck. An agent blocked on a permission or an input prompt is
 * answered in its console, so the row opens the console with the keyboard
 * already in it.
 *
 * Everything else lands where it always did — the task's panel for a row that
 * is about a task, the session's for one that is only about a session — since
 * a death or a stall is something to read rather than something to answer. A
 * pull request that is ready to merge is one of the latter: its session's
 * panel links to the request, where the merge is done.
 *
 * The screen it is answered *from* matters as well as its params: the list
 * carries onto every screen (`attention-alerts.tsx`), and the sessions one
 * reads `?goal=`/`?task=` as its own filters rather than as panels. The
 * `…From` helpers are where that is settled.
 */
/**
 * Where an attention row navigates: an internal route, for `<Link to={…}>`
 * and `useNavigate()` alike.
 */
export type AttentionNavTarget = {
  pathname?: string
  search: string
  replace?: boolean
  /** The named host whose agents the target opens; the pick must then include it. */
  agentsOf?: Host
}

export function attentionTarget(
  item: AttentionItem,
  current: URLSearchParams,
  pathname: string,
): AttentionNavTarget {
  const { host } = item
  if (item.recovery) return recoveryTarget(item.recovery, host, current, pathname)
  const { session, sessionReason, taskId } = item
  if (session && (sessionReason === "waiting_permission" || sessionReason === "waiting_input")) {
    return sessionTerminalFrom(pathname, current, session.id, host)
  }
  // A row the task itself put on the list is the task's, whatever session sits
  // on it; one that is only a session's opens that session.
  if (!item.taskReason && session) return sessionPanelFrom(pathname, current, session.id, host)
  return taskPanelFrom(pathname, current, taskId ?? item.id, host)
}

/**
 * What to run when a row's target is followed: a link to the agents of a named
 * host sets the pick to All hosts when that host is outside the pick, so the
 * screen has the host's chip to pick.
 */
export function useFollowAttention(): (target: AttentionNavTarget) => void {
  const pick = useSettingsStore((state) => state.hostFilter)
  const setPick = useSettingsStore((state) => state.setHostFilter)
  return useCallback(
    (target) => {
      if (target.agentsOf && pick !== "" && pick !== target.agentsOf) setPick("")
    },
    [pick, setPick],
  )
}

/**
 * Where a recovery row's own target (`GET /v1/attention`) lands: the console
 * of the session it names, the panel of the task it names, or the
 * Repositories screen for a configuration blocker — the daemon names a
 * section of it, but there is no per-section route to deep-link into yet.
 */
function recoveryTarget(
  item: AttentionItemDto,
  host: Host,
  current: URLSearchParams,
  pathname: string,
): AttentionNavTarget {
  switch (item.target.kind) {
    case "console":
      return sessionTerminalFrom(pathname, current, item.target.session_id, host)
    case "goal": {
      const [path, search] = paths.goal(item.target.goal_id, host).split("?")
      return { pathname: path, search: search ? `?${search}` : "" }
    }
    case "task":
      return taskPanelFrom(pathname, current, item.target.task_id, host)
    case "pull_request": {
      const [path, search] = paths
        .pullRequest(item.target.pull_request_id, current, host)
        .split("?")
      return { pathname: path, search: `?${search}` }
    }
    case "settings":
      // The agents of a named host are read on that host's chip.
      if (item.target.section === "agents") {
        return host === LOCAL_HOST
          ? { pathname: "/agents", search: "" }
          : { pathname: "/agents", search: `?host=${host}`, agentsOf: host }
      }
      return { pathname: paths.repositories(), search: "" }
  }
}

/**
 * The agent behind a row's session: its seat, or for a step agent the
 * workflow column it staffs.
 */
export function agentLabel(item: AttentionItem): string {
  if (!item.session) return ""
  return sessionColumn(item.session, item.task, item.goal) ?? seatLabel(item.session.seat)
}

/**
 * What the row is about, in one line: the task the agent was working on, or —
 * for an orchestrator, which has none — its seat and the goal it is planning.
 *
 * The task is named even when the task list did not carry it, by the short id
 * every other mention of a task uses: a row with no subject at all is a row
 * nobody can tell apart from the next one.
 */
export function attentionSubject(item: AttentionItem): string {
  if (item.recovery) return item.recovery.summary
  if (item.task) return item.task.title
  if (item.taskId) return `Task ${shortId(item.taskId)}`
  // A pull request's session works for no goal: the request is its subject.
  const pullRequestId = item.session?.pull_request_id
  if (pullRequestId) {
    return `Pull request · ${shortId(pullRequestId)}`
  }
  return item.session
    ? `${agentLabel(item)} · ${item.goal?.title ?? `Goal ${shortId(item.goalId)}`}`
    : `Goal ${shortId(item.goalId)}`
}

/**
 * Why the row is on the list, spelled out under its subject: who is asking and
 * what for.
 *
 * The session's reason leads where there is one — it is the live half of a
 * folded row, and which of the three agents raised it is the other half of
 * knowing whether the row is yours to answer. A row that is only a task's says
 * so instead; the badge beside it already names the status.
 */
export function attentionDetail(item: AttentionItem): string {
  if (item.recovery) return item.recovery.required_action
  if (item.session && item.sessionReason) {
    const hint = SESSION_ATTENTION_META[item.sessionReason].hint
    // A task-less session already says its seat in the subject.
    return item.taskId ? `${agentLabel(item)} · ${hint}` : hint
  }
  const reason = item.taskReason
  return `Task · ${reason === "stalled" ? STALLED_META.hint : TASK_STATUS_META.failed.hint}`
}

/**
 * A recovery row's own short title, read by its producer and cause rather
 * than guessed from a task or session reason it carries neither of — a
 * `pull_request` item's own two shapes get their own words, since
 * "configuration" and "unknown" mean something different for every other
 * producer's item under the same cause.
 */
const RECOVERY_CAUSE_TITLE: Record<AttentionCause, string> = {
  access: "Sign-in needed",
  quota: "Quota exhausted",
  configuration: "Configuration needed",
  resource: "Resource exhausted",
  unknown: "Needs attention",
}

export function recoveryHeadline(item: AttentionItemDto): string {
  if (item.producer === "pull_request") {
    return item.reason === "configuration" ? "Review needed" : "Ready to merge"
  }
  return RECOVERY_CAUSE_TITLE[item.reason]
}

/**
 * What a recovery row's blocker affects, as one line of labels — null for
 * every other row, which already names its one task or session in the
 * subject. A grouped item (a model exhausted on three sessions, a machine
 * resource shortage across two tasks) would otherwise say only the first
 * of them.
 */
export function attentionAffected(item: AttentionItem): string | null {
  if (!item.recovery || item.recovery.affected.length === 0) return null
  return item.recovery.affected.map((subject) => subject.label).join(", ")
}

/** One affected session, reachable on its own. */
interface AttentionAffectedLink {
  id: string
  label: string
  to: { pathname?: string; search: string }
}

/**
 * Every affected session a grouped recovery item names, each with its own
 * link to its console — not only the item's single `target`, which a
 * group of several can answer with only one of them. `null` below two
 * entries: a lone session is already where the row's own link goes, and a
 * second line saying the same thing is noise.
 */
export function attentionAffectedLinks(
  item: AttentionItem,
  current: URLSearchParams,
  pathname: string,
): AttentionAffectedLink[] | null {
  if (item.recovery?.producer === "recovery") return null
  const sessions = item.recovery?.affected.filter((subject) => subject.kind === "session") ?? []
  if (sessions.length < 2) return null
  return sessions.map((subject) => ({
    id: subject.id,
    label: subject.label,
    to: sessionTerminalFrom(pathname, current, subject.id, item.host),
  }))
}
