/**
 * What is stuck, in one query, for the strip above the board — and, from the
 * same query, for the badges on the board itself ({@link useBoardAttention})
 * and for the count the shell carries onto every other screen (see
 * `attention-alerts.tsx`).
 *
 * The orchestrator's first question — "which tasks are stalled or failed, which
 * agents died or are waiting on me?" — used to need every goal opened one by
 * one. It is three list queries, all of them shared keys the SSE dispatcher
 * already invalidates, so the answer stays live without anything here
 * subscribing to events.
 *
 * All three are read unfiltered on purpose: the board's status filter narrows
 * the lanes, never this — a stuck task the filter hides is exactly the one the
 * strip exists to keep in sight.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import {
  SESSION_ATTENTION_META,
  type SessionAttention,
  sessionAttention,
} from "@/features/sessions/session-display"
import { STALLED_META, TASK_STATUS_META, taskListQueryOptions } from "@/features/tasks"
import { ROLE_LABELS, shortId } from "@/lib/format"
import {
  goalThreadTo,
  sessionPanelFrom,
  sessionTerminalFrom,
  taskConversationFrom,
  taskPanelFrom,
} from "@/routes/paths"

import { goalsQueryOptions } from "./queries"

/** Why a task is on the list, strongest first. */
export type AttentionReason = "failed" | "stalled"

/**
 * Whether this task wants the user, and what for.
 *
 * A task in `changes_requested` is deliberately not one of them: the reviewer
 * has spoken and the daemon resumes the engineer itself, so what that task
 * waits on is an agent, not a person. A resume that does not happen shows up
 * as the session's own `disconnected` or `stalled` flag, which is where the
 * daemon decides a human is wanted.
 *
 * `stalled` is checked last because it is a flag *on top of* a status: the
 * task's column mirrors any of its sessions carrying the daemon's `stalled`
 * flag, and comes down when that session's does, so a task that also failed is
 * reported as failed.
 */
export function taskAttentionReason(task: TaskDto): AttentionReason | null {
  if (task.status === "failed") return "failed"
  return task.stalled ? "stalled" : null
}

/**
 * One row of the list: everything that wants a person about one task, or about
 * one session that belongs to no task.
 *
 * A task and the session running it can both be stuck at once — a `failed`
 * task under an agent that reported an error, a stalled task under a stalled
 * session — and they are one thing gone wrong, not two. So the row is the
 * task, and the session it carries is a second badge on it rather than a
 * second row saying the same thing a line apart. A planner's session, which
 * belongs to no task, is a row in its own right.
 */
export interface AttentionItem {
  /**
   * What identifies the row: the task it is about, or the task-less session
   * that is one. Its React key, and what the toast that announced it
   * remembers.
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
}

interface Attention {
  /** One row per stuck task or task-less session, most recently moved first. */
  items: AttentionItem[]
  isPending: boolean
  /** The first of the three queries that failed, if any. */
  error: unknown
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

export function useAttention(): Attention {
  const goals = useQuery(goalsQueryOptions())
  const tasks = useQuery(taskListQueryOptions())
  // Unfiltered, and narrowed by `sessionAttention` below rather than by the
  // daemon: the key is the one the sessions screen already holds, so the extra
  // rows usually cost no extra request, where `GET /v1/sessions?attention=true`
  // would be a second list of its own. Filtering here is also what keeps the
  // rule — "the daemon raised a reason for it" — in one place for both this
  // strip and `ariadne attention`.
  const sessions = useQuery(sessionsQueryOptions())

  const items = useMemo(
    () => collectAttention(goals.data, tasks.data, sessions.data),
    [goals.data, tasks.data, sessions.data],
  )

  const error = goals.error ?? tasks.error ?? sessions.error

  return {
    items,
    isPending: goals.isPending || tasks.isPending || sessions.isPending,
    error,
    partial: error !== null && items.length > 0,
    refetch: () => {
      void goals.refetch()
      void tasks.refetch()
      void sessions.refetch()
    },
  }
}

/**
 * The tasks and the sessions folded into one list of rows, newest first.
 *
 * Which sessions belong is {@link sessionAttention}'s call, not this list's,
 * so the strip and `ariadne attention` include and label the same ones.
 *
 * A flagged session lands on its task's row rather than on one of its own —
 * the pair is one thing gone wrong (see {@link AttentionItem}) — and a task
 * with several of them keeps the most recently raised, which is the one the
 * user has not seen yet and the one the board's card shows. The row is dated
 * by whichever of its two reasons moved last, since that is what the list is
 * ordered by and what "last moved" claims on the row itself.
 *
 * A goal the goals list did not carry does not drop its rows: a task or a
 * session can outlive its goal falling out of a filtered list, and hiding it
 * would hide exactly the row the strip exists for — the row names the goal by
 * its short id instead.
 */
export function collectAttention(
  goals: GoalDto[] | undefined,
  tasks: TaskDto[] | undefined,
  sessions: SessionDto[] | undefined,
): AttentionItem[] {
  const goalsById = new Map((goals ?? []).map((goal) => [goal.id, goal]))
  const tasksById = new Map((tasks ?? []).map((task) => [task.id, task]))
  /** Keyed by what identifies a row, which is what folds the two kinds. */
  const rows = new Map<string, AttentionItem>()
  /**
   * When each row's folded session raised its reason, while the newest of them
   * is still being picked — the row's own stamp is the later of that and the
   * task's, so it cannot be compared against.
   */
  const flaggedAt = new Map<string, string>()

  for (const task of tasks ?? []) {
    const reason = taskAttentionReason(task)
    if (!reason) continue
    rows.set(task.id, {
      id: task.id,
      goalId: task.goal_id,
      goal: goalsById.get(task.goal_id),
      at: task.updated_at,
      taskId: task.id,
      task,
      taskReason: reason,
      session: undefined,
      sessionReason: null,
    })
  }

  for (const session of sessions ?? []) {
    const reason = sessionAttention(session)
    if (!reason) continue
    const at = sessionAttentionAt(session)
    const taskId = session.task_id ?? null
    const key = taskId ?? session.id
    const row = rows.get(key)
    // Not the newest of this task's flagged sessions: the row already carries
    // one raised more recently, which is the one the user has not seen yet.
    const held = flaggedAt.get(key)
    if (held && held.localeCompare(at) >= 0) continue
    flaggedAt.set(key, at)
    rows.set(key, {
      id: key,
      goalId: session.goal_id,
      goal: goalsById.get(session.goal_id),
      // The row is as recent as its most recent reason, whichever raised it.
      at: row && row.at.localeCompare(at) > 0 ? row.at : at,
      taskId,
      task: row?.task ?? (taskId ? tasksById.get(taskId) : undefined),
      taskReason: row?.taskReason ?? null,
      session,
      sessionReason: reason,
    })
  }

  return [...rows.values()].sort((a, b) => b.at.localeCompare(a.at) || a.id.localeCompare(b.id))
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
  return session.attention_since ?? session.ended_at ?? session.created_at
}

/**
 * Which cards on the board are asking for a person, indexed by what the agent
 * was working on.
 *
 * The strip answers "what is waiting for me"; this answers the same question
 * where the work itself is, so a blocked task is recognisable in its lane
 * without reading the list above it. It is the sessions query the strip
 * already holds — same shared key, so the board costs no extra request and
 * goes live off the same SSE invalidation — narrowed to the one thing a card
 * can show.
 */
export interface BoardAttention {
  /** Task id → why one of its sessions wants a person. */
  byTask: Map<string, SessionAttention>
  /**
   * Goal id → why its planner wants a person. Kept apart from `byTask`
   * because a planner session belongs to no task and so has no card of its
   * own; its lane header is the only place it can be seen.
   */
  byGoal: Map<string, SessionAttention>
}

export function useBoardAttention(): BoardAttention {
  const sessions = useQuery(sessionsQueryOptions())
  return useMemo(() => collectBoardAttention(sessions.data), [sessions.data])
}

/**
 * The flagged sessions folded onto their subjects.
 *
 * A task can have several sessions flagged at once — an engineer waiting on a
 * permission prompt while last round's reviewer sits disconnected — and a card
 * has room for one badge, so the most recently raised reason wins: it is the
 * one the strip lists first, and the one the user has not seen yet.
 */
export function collectBoardAttention(sessions: SessionDto[] | undefined): BoardAttention {
  const byTask = new Map<string, Flagged>()
  const byGoal = new Map<string, Flagged>()

  for (const session of sessions ?? []) {
    const reason = sessionAttention(session)
    if (!reason) continue
    const at = sessionAttentionAt(session)
    // A session with no task is a planner's, and lands on its goal's lane.
    const index = session.task_id ? byTask : byGoal
    const key = session.task_id ?? session.goal_id
    const held = index.get(key)
    if (!held || held.at.localeCompare(at) < 0) index.set(key, { reason, at })
  }

  return { byTask: reasons(byTask), byGoal: reasons(byGoal) }
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
 * agent is stuck. A `waiting_user` agent asked its question in a thread and is
 * answered by a message, so the row opens that thread with the box focused and
 * addressed to it — the session's own panel would show a pane with nothing to
 * type into. An agent blocked on a permission or an input prompt is the
 * opposite: the answer is a keystroke in its pane, so the row opens the
 * terminal with the keyboard already in it.
 *
 * Everything else lands where it always did — the task's panel for a row that
 * is about a task, the session's for one that is only about a session — since
 * a death or a stall is something to read rather than something to answer.
 *
 * The screen it is answered *from* matters as well as its params: the list
 * carries onto every screen (`attention-alerts.tsx`), and the sessions one
 * reads `?goal=`/`?task=` as its own filters rather than as panels. The
 * `…From` helpers are where that is settled.
 */
export function attentionTarget(
  item: AttentionItem,
  current: URLSearchParams,
  pathname: string,
): { pathname?: string; search: string } {
  const { session, sessionReason, taskId } = item
  if (session && sessionReason === "waiting_user") {
    // A planner belongs to no task, so its question is in the goal's thread.
    return taskId
      ? taskConversationFrom(pathname, current, taskId, session.profile_id)
      : goalThreadTo(current, item.goalId, session.profile_id)
  }
  if (session && (sessionReason === "waiting_permission" || sessionReason === "waiting_input")) {
    return sessionTerminalFrom(pathname, current, session.id)
  }
  // A row the task itself put on the list is the task's, whatever session sits
  // on it; one that is only a session's opens that session.
  if (!item.taskReason && session) return sessionPanelFrom(pathname, current, session.id)
  return taskPanelFrom(pathname, current, taskId ?? item.id)
}

/**
 * What the row is about, in one line: the task the agent was working on, or —
 * for a planner, which has none — its role and the goal it is planning.
 *
 * The task is named even when the task list did not carry it, by the short id
 * every other mention of a task uses: a row with no subject at all is a row
 * nobody can tell apart from the next one.
 */
export function attentionSubject(item: AttentionItem): string {
  if (item.task) return item.task.title
  if (item.taskId) return `Task ${shortId(item.taskId)}`
  return item.session
    ? `${ROLE_LABELS[item.session.role]} · ${item.goal?.title ?? `Goal ${shortId(item.goalId)}`}`
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
  if (item.session && item.sessionReason) {
    const hint = SESSION_ATTENTION_META[item.sessionReason].hint
    // A task-less session already says its role in the subject.
    return item.taskId ? `${ROLE_LABELS[item.session.role]} · ${hint}` : hint
  }
  const reason = item.taskReason
  return `Task · ${reason === "stalled" ? STALLED_META.hint : TASK_STATUS_META.failed.hint}`
}
