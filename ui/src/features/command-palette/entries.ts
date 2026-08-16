/**
 * What the palette can take you to, and where each of those lands.
 *
 * Everything the palette shows for an entity is built here, away from the
 * dialog: the row's text, what a search matches it by, and the target it
 * navigates to. The palette itself is then just a list.
 *
 * Targets are *descriptions*, not URLs, because where a task or a session opens
 * depends on the screen the palette was opened over — a task panel stacks on
 * whatever is showing, a session opens inside its own panel — so the URL can
 * only be built at the moment of the pick, by {@link paletteTargetTo} against
 * the live search params.
 */

import type { GoalDto, ProfileDto, SessionDto, TaskDto } from "@/api"
import { roleLabel } from "@/features/profiles/profile-labels"
import { ROLE_LABELS } from "@/features/sessions/session-display"
import { shortId } from "@/lib/ids"
import { paths, taskPanelTo, taskSessionPanelTo } from "@/routes/paths"

/** Where a palette entry goes when it is picked. */
export type PaletteTarget =
  | { kind: "goal"; goalId: string }
  | { kind: "task"; taskId: string }
  | { kind: "session"; sessionId: string; goalId: string; taskId: string | null }
  | { kind: "page"; path: string }

export interface PaletteEntry {
  /**
   * cmdk's item value: what the fuzzy filter scores, and what tells two rows
   * apart. The id is part of it so that a search for an id finds the row and
   * two tasks with the same title stay two rows.
   */
  value: string
  /** The row's own text. */
  label: string
  /** Secondary text on the right of the row: the id, the branch, the role. */
  detail?: string
  /** Also searchable, without being written on the row. */
  keywords: string[]
  target: PaletteTarget
}

export interface PaletteSource {
  goals: GoalDto[] | undefined
  tasks: TaskDto[] | undefined
  sessions: SessionDto[] | undefined
  profiles: ProfileDto[] | undefined
}

/** One group of rows per entity, in the order the palette lists them. */
export interface PaletteEntries {
  goals: PaletteEntry[]
  tasks: PaletteEntry[]
  sessions: PaletteEntry[]
  profiles: PaletteEntry[]
}

export function buildPaletteEntries({
  goals,
  tasks,
  sessions,
  profiles,
}: PaletteSource): PaletteEntries {
  const goalTitles = new Map((goals ?? []).map((goal) => [goal.id, goal.title]))
  const taskTitles = new Map((tasks ?? []).map((task) => [task.id, task.title]))

  return {
    goals: (goals ?? []).map((goal) => ({
      value: `${goal.title} ${goal.id}`,
      label: goal.title,
      detail: shortId(goal.id),
      keywords: [goal.status],
      target: { kind: "goal", goalId: goal.id },
    })),

    tasks: (tasks ?? []).map((task) => ({
      value: `${task.title} ${task.id}`,
      label: task.title,
      // The branch is what a task is called outside Ariadne, so it is both
      // written on the row and searchable.
      detail: task.branch,
      keywords: [task.status, task.branch, goalTitles.get(task.goal_id) ?? ""],
      target: { kind: "task", taskId: task.id },
    })),

    // A session has no name of its own: it is "the reviewer of <task>", and
    // that is also how somebody looking for one describes it.
    sessions: (sessions ?? []).map((session) => {
      const of =
        (session.task_id ? taskTitles.get(session.task_id) : undefined) ??
        goalTitles.get(session.goal_id)
      return {
        value: `${ROLE_LABELS[session.role]} ${of ?? ""} ${session.id}`,
        label: of ? `${ROLE_LABELS[session.role]} · ${of}` : ROLE_LABELS[session.role],
        detail: shortId(session.id),
        keywords: [session.status, session.tmux_session, session.agent_kind],
        target: {
          kind: "session",
          sessionId: session.id,
          goalId: session.goal_id,
          taskId: session.task_id ?? null,
        },
      }
    }),

    profiles: (profiles ?? []).map((profile) => ({
      value: `${profile.name} ${profile.id}`,
      label: profile.name,
      detail: roleLabel(profile.role),
      keywords: [profile.role, profile.agent_kind ?? "", profile.model ?? ""],
      target: { kind: "page", path: paths.profiles() },
    })),
  }
}

/**
 * The route a picked entry navigates to, against the params of the screen the
 * palette was opened over — which is what makes a task stack on the goal that
 * is already open rather than replacing it.
 *
 * A planner session belongs to no task, and the goal panel only exists on the
 * board, so that one leaves the current screen for it: the same rule the
 * sessions list follows.
 */
export function paletteTargetTo(
  target: PaletteTarget,
  search: URLSearchParams,
): string | { search: string } {
  switch (target.kind) {
    case "goal":
      return paths.goal(target.goalId)
    case "task":
      return taskPanelTo(search, target.taskId)
    case "session":
      return target.taskId
        ? taskSessionPanelTo(search, target.taskId, target.sessionId)
        : paths.goalSession(target.goalId, target.sessionId)
    case "page":
      return target.path
  }
}
