/**
 * What order the board's groups go in, and what each goal's bar counts.
 *
 * Both are the board's answer to the same complaint: it was ordered by id
 * alone, so an old active goal sat under every finished one, and a goal said
 * only how many tasks it had — which is the one number that never changes once
 * the orchestrator is done.
 *
 * Pure, and apart from `goal-board.tsx`, because the ordering is the board's
 * whole reading order and the counters are the only thing a folded goal still
 * says.
 */

import type { HostRow, TaskDto, TaskStatus } from "@/api"
import { plural } from "@/lib/format"

/** What the board orders a group by: one goal with its tasks, or one task with no goal. */
export interface GroupOrder {
  /** It asks for a person: a flagged agent, a failed or a stalled task. */
  needsAttention: boolean
  /** A finished or cancelled goal, or a task with no goal in one of those states. */
  terminal: boolean
  updatedAt: string
  id: string
}

/**
 * Which band a group belongs to, lowest first: what is asking for a person,
 * then what is still moving, then what is done with.
 *
 * A band rather than a full ordering because the three answer different
 * questions — the first is "what do I have to do", the second "what is
 * running", the third "what happened" — and only the first two are why the
 * board is open.
 */
function band(order: GroupOrder): number {
  if (order.needsAttention) return 0
  return order.terminal ? 2 : 1
}

/**
 * The groups in the order the board lays them out: by band, then by when each
 * one last moved, newest first.
 *
 * `updated_at` rather than the id the list arrives sorted by: a goal that just
 * went active, and a goal that just finished, are both more interesting than
 * one created after them and untouched since. The id breaks the ties, so the
 * order is total and two renders of the same board agree. A task with no goal
 * takes the same rule by its own state.
 */
export function orderGroups<G>(groups: readonly G[], orderOf: (group: G) => GroupOrder): G[] {
  // By the row itself: two hosts may each have a goal of one id.
  const orders = new Map(groups.map((group) => [group, orderOf(group)]))
  return [...groups].sort((a, b) => {
    const x = orders.get(a)
    const y = orders.get(b)
    if (!x || !y) return 0
    const byBand = band(x) - band(y)
    if (byBand !== 0) return byBand
    return y.updatedAt.localeCompare(x.updatedAt) || y.id.localeCompare(x.id)
  })
}

/** What a goal line says about its tasks, and what a folded goal says at all. */
interface GoalCounts {
  /**
   * Everything that is in the pipeline or has been through it — the whole goal
   * minus what was cancelled, which is what "N finished of how many" is out of.
   * A cancelled task was taken out of the count on purpose, so leaving it in
   * would mean a finished goal never reads as finished.
   */
  pipeline: number
  finished: number
  /** Retry candidates, outlined in danger. */
  failed: number
  /** A session stopped moving before it could report a task failure. */
  stalled: number
  /** Not started — waiting on a dependency, or on an agent session. */
  waiting: number
  cancelled: number
}

const WAITING_STATUSES: readonly TaskStatus[] = ["pending", "ready"]

export function goalCounts(tasks: readonly TaskDto[]): GoalCounts {
  const counts: GoalCounts = {
    pipeline: 0,
    finished: 0,
    failed: 0,
    stalled: 0,
    waiting: 0,
    cancelled: 0,
  }
  for (const task of tasks) {
    if (task.status === "cancelled") {
      counts.cancelled += 1
      continue
    }
    counts.pipeline += 1
    if (task.status === "finished") counts.finished += 1
    else if (task.status === "failed") counts.failed += 1
    else if (task.stalled) counts.stalled += 1
    else if (WAITING_STATUSES.includes(task.status)) counts.waiting += 1
  }
  return counts
}

/**
 * The goal in one line: "3/7 finished · 1 failed · 1 waiting" while it is
 * running, "7 tasks finished · 1 cancelled" once it is done.
 *
 * Only what is worth saying: a goal with nothing failed does not say "0
 * failed", and a goal that is all the way through says so in words rather than
 * as a fraction of itself. It is the whole of a folded goal's row, so it has
 * to read as a sentence about the goal and not as a row of counters.
 */
export function goalSummary(tasks: readonly TaskDto[]): string {
  const counts = goalCounts(tasks)
  if (counts.pipeline === 0 && counts.cancelled === 0) return "No tasks"

  const parts: string[] = []
  if (counts.pipeline > 0) {
    parts.push(
      counts.finished === counts.pipeline
        ? `${plural(counts.pipeline, "task")} finished`
        : `${counts.finished}/${counts.pipeline} finished`,
    )
  }
  if (counts.failed > 0) parts.push(`${counts.failed} failed`)
  if (counts.stalled > 0) parts.push(`${counts.stalled} stalled`)
  if (counts.waiting > 0) parts.push(`${counts.waiting} waiting`)
  if (counts.cancelled > 0) parts.push(`${counts.cancelled} cancelled`)
  return parts.join(" · ")
}

/**
 * The tasks with no goal, in the order the daemon listed them, each a group
 * of one row: narrowed by the board's active-only toggle and its title search
 * the way the goals are.
 */
export function goallessTasks<T extends TaskDto>(
  rows: readonly HostRow<T>[],
  activeOnly: boolean,
  query: string,
): HostRow<T>[] {
  const needle = query.trim().toLowerCase()
  return rows.filter(
    (row) =>
      !row.goal_id &&
      (!activeOnly || (row.status !== "finished" && row.status !== "cancelled")) &&
      (!needle || row.title.toLowerCase().includes(needle)),
  )
}
