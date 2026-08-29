/**
 * What order the board's lanes go in, and what each lane's header counts.
 *
 * Both are the board's answer to the same complaint: it was ordered by id
 * alone, so an old active goal sat under every finished one, and a lane said
 * only how many tasks it had — which is the one number that never changes once
 * the planner is done.
 *
 * Pure, and apart from `goal-swimlanes.tsx`, because the ordering is the
 * board's whole reading order and the counters are the only thing a folded
 * lane still says.
 */

import type { GoalDto, TaskDto, TaskStatus } from "@/api"
import { plural } from "@/lib/format"
import { isTerminalGoalStatus } from "./status"

/**
 * Which band a lane belongs to, lowest first: what is asking for a person,
 * then what is still moving, then what is done with.
 *
 * A band rather than a full ordering because the three answer different
 * questions — the first is "what do I have to do", the second "what is
 * running", the third "what happened" — and only the first two are why the
 * board is open.
 */
function laneBand(goal: GoalDto, needsAttention: boolean): number {
  if (needsAttention) return 0
  return isTerminalGoalStatus(goal.status) ? 2 : 1
}

/**
 * The goals in the order the board lays them out: by band, then by when each
 * one last moved, newest first.
 *
 * `updated_at` rather than the id the list arrives sorted by: a goal that just
 * went active, and a goal that just finished, are both more interesting than
 * one created after them and untouched since. The id breaks the ties, so the
 * order is total and two renders of the same board agree.
 */
export function orderLanes(
  goals: readonly GoalDto[],
  needsAttention: (goal: GoalDto) => boolean,
): GoalDto[] {
  const bands = new Map(goals.map((goal) => [goal.id, laneBand(goal, needsAttention(goal))]))
  return [...goals].sort((a, b) => {
    const band = (bands.get(a.id) ?? 0) - (bands.get(b.id) ?? 0)
    if (band !== 0) return band
    return b.updated_at.localeCompare(a.updated_at) || b.id.localeCompare(a.id)
  })
}

/** What a lane header says about its tasks, and what a folded lane says at all. */
interface LaneCounts {
  /**
   * Everything that is in the pipeline or has been through it — the whole lane
   * minus what was cancelled, which is what "N merged of how many" is out of.
   * A cancelled task was taken out of the count on purpose, so leaving it in
   * would mean a finished goal never reads as finished.
   */
  pipeline: number
  merged: number
  /** Retry candidates: they stay in the Pending column, outlined in danger. */
  failed: number
  /** Not started — waiting on a dependency, or on an engineer session. */
  waiting: number
  cancelled: number
}

const WAITING_STATUSES: readonly TaskStatus[] = ["pending", "ready"]

function countLane(tasks: readonly TaskDto[]): LaneCounts {
  const counts: LaneCounts = { pipeline: 0, merged: 0, failed: 0, waiting: 0, cancelled: 0 }
  for (const task of tasks) {
    if (task.status === "cancelled") {
      counts.cancelled += 1
      continue
    }
    counts.pipeline += 1
    if (task.status === "merged") counts.merged += 1
    else if (task.status === "failed") counts.failed += 1
    else if (WAITING_STATUSES.includes(task.status)) counts.waiting += 1
  }
  return counts
}

/**
 * The lane in one line: "3/7 merged · 1 failed · 1 waiting" while it is
 * running, "7 tasks merged · 1 cancelled" once it is done.
 *
 * Only what is worth saying: a lane with nothing failed does not say "0
 * failed", and a lane that is all the way through says so in words rather than
 * as a fraction of itself. It is the whole of a folded lane's header, so it
 * has to read as a sentence about the goal and not as a row of counters.
 */
export function laneSummary(tasks: readonly TaskDto[]): string {
  const counts = countLane(tasks)
  if (counts.pipeline === 0 && counts.cancelled === 0) return "No tasks"

  const parts: string[] = []
  if (counts.pipeline > 0) {
    parts.push(
      counts.merged === counts.pipeline
        ? `${plural(counts.pipeline, "task")} merged`
        : `${counts.merged}/${counts.pipeline} merged`,
    )
  }
  if (counts.failed > 0) parts.push(`${counts.failed} failed`)
  if (counts.waiting > 0) parts.push(`${counts.waiting} waiting`)
  if (counts.cancelled > 0) parts.push(`${counts.cancelled} cancelled`)
  return parts.join(" · ")
}
