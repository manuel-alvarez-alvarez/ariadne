/**
 * What order the board's lanes go in, and what each lane's header counts.
 *
 * Both are the board's answer to the same complaint: it was ordered by id
 * alone, so an old active goal sat under every finished one, and a lane said
 * only how many tasks it had — which is the one number that never changes once
 * the orchestrator is done.
 *
 * Pure, and apart from `goal-swimlanes.tsx`, because the ordering is the
 * board's whole reading order and the counters are the only thing a folded
 * lane still says.
 */

import type { GoalDto, TaskDto, TaskStatus } from "@/api"
import { TASK_STATUS_META } from "@/features/tasks"
import { plural } from "@/lib/format"
import { isStillPlanning, isTerminalGoalStatus } from "./status"

/** One column of a lane: what its header says, and the key its cards are filed under. */
interface LaneColumn {
  key: string
  label: string
  /** What the column means, for its header's tooltip. */
  hint: string
  /** Solid dot classes, from the status ramp. */
  dot: string
}

/** Where a stepped lane files the tasks that sit in a column, apart from the status keys. */
function stepKey(id: string): string {
  return `step:${id}`
}

/**
 * The columns a goal's lane draws, in order.
 *
 * Every goal draws Pending, its workflow columns, then Done.
 */
export function laneColumns(goal: Pick<GoalDto, "steps">): LaneColumn[] {
  return [
    {
      key: "pending",
      label: TASK_STATUS_META.pending.label,
      hint: TASK_STATUS_META.pending.hint,
      dot: TASK_STATUS_META.pending.dot,
    },
    ...goal.steps.map((step) => ({
      key: stepKey(step.id),
      label: step.title,
      hint: step.description || step.title,
      dot: TASK_STATUS_META.in_progress.dot,
    })),
    {
      key: "finished",
      label: "Done",
      hint: TASK_STATUS_META.finished.hint,
      dot: TASK_STATUS_META.finished.dot,
    },
  ]
}

/**
 * The key of the column a task's card sits in, or null for a task off the
 * board (cancelled).
 *
 * A goal still being planned holds all of its tasks in the first column,
 * `pending` and `ready` alike: nothing under it has been handed to an
 * agent, so a card further along would say a task is moving when it is
 * waiting on the orchestrator.
 *
 * A failed task lands in that first column too, whatever the goal is doing:
 * the one thing anybody does with a failure is retry it, and a retry puts it
 * back exactly there. Its card is outlined in danger and badged `Failed`.
 *
 * On a stepped goal, a task under way sits in the column of its `step`; one
 * whose step the goal does not list sits in the first step's column.
 */
export function laneColumnOf(
  task: TaskDto,
  goal: Pick<GoalDto, "status" | "steps">,
): string | null {
  if (task.status === "cancelled") return null
  if (task.status === "failed" || isStillPlanning(goal.status)) return "pending"
  if (task.status === "pending" || task.status === "ready") return "pending"
  if (task.status === "finished") return "finished"
  const step = goal.steps.find((one) => one.id === task.step) ?? goal.steps[0]
  return step ? stepKey(step.id) : status
}

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
   * minus what was cancelled, which is what "N finished of how many" is out of.
   * A cancelled task was taken out of the count on purpose, so leaving it in
   * would mean a finished goal never reads as finished.
   */
  pipeline: number
  finished: number
  /** Retry candidates: they stay in the Pending column, outlined in danger. */
  failed: number
  /** A session stopped moving before it could report a task failure. */
  stalled: number
  /** Not started — waiting on a dependency, or on an author session. */
  waiting: number
  cancelled: number
}

const WAITING_STATUSES: readonly TaskStatus[] = ["pending", "ready"]

export function laneCounts(tasks: readonly TaskDto[]): LaneCounts {
  const counts: LaneCounts = {
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
 * The lane in one line: "3/7 finished · 1 failed · 1 waiting" while it is
 * running, "7 tasks finished · 1 cancelled" once it is done.
 *
 * Only what is worth saying: a lane with nothing failed does not say "0
 * failed", and a lane that is all the way through says so in words rather than
 * as a fraction of itself. It is the whole of a folded lane's header, so it
 * has to read as a sentence about the goal and not as a row of counters.
 */
export function laneSummary(tasks: readonly TaskDto[]): string {
  const counts = laneCounts(tasks)
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
