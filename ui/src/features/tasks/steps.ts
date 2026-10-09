/**
 * A stepped goal's columns, as every screen names them.
 *
 * A goal run by a workflow carries its columns in `steps`, in order; a task of
 * it carries the id of the column it is in, and each agent the id of the
 * column it staffs. The wire only has ids, so every screen that names a column
 * asks here, and a column the goal no longer lists falls back to its id.
 */

import type { GoalDto, SessionDto, TaskDto, WorkflowStepDto } from "@/api"

/** The column an id names on the goal, if the goal has it. */
export function findStep(
  steps: readonly WorkflowStepDto[] | undefined,
  id: string | null | undefined,
): WorkflowStepDto | undefined {
  return id ? steps?.find((step) => step.id === id) : undefined
}

/** A column's title, or its id where the goal does not list it. */
export function stepTitle(steps: readonly WorkflowStepDto[] | undefined, id: string): string {
  return findStep(steps, id)?.title ?? id
}

/**
 * The column a session of seat `agent` staffs, by title — what a step agent is
 * called wherever a seat would name an author or a reviewer. Undefined for any
 * other seat, and where the task is not at hand to say which column it is.
 */
export function sessionColumn(
  session: Pick<SessionDto, "seat" | "task_agent_id">,
  task: Pick<TaskDto, "agents"> | undefined,
  goal: Pick<GoalDto, "steps"> | undefined,
): string | undefined {
  if (session.seat !== "agent") return undefined
  const step = task?.agents.find((agent) => agent.id === session.task_agent_id)?.step
  return step ? stepTitle(goal?.steps, step) : undefined
}
