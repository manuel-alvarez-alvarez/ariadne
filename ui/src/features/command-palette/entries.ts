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
import { type AttentionItem, attentionSubject, attentionTarget } from "@/features/goals/attention"
import { roleLabel } from "@/features/profiles/profile-labels"
import { SESSION_ATTENTION_META } from "@/features/sessions/session-display"
import { STALLED_META, TASK_STATUS_META } from "@/features/tasks"
import { ROLE_LABELS, shortId } from "@/lib/format"
import { paths, taskPanelFrom, taskSessionPanelFrom } from "@/routes/paths"

/** Where a palette entry goes when it is picked. */
type PaletteTarget =
  | { kind: "goal"; goalId: string }
  | { kind: "task"; taskId: string }
  | { kind: "session"; sessionId: string; goalId: string; taskId: string | null }
  /**
   * A row of the attention list, which decides where it goes itself: the
   * thread a question was asked in, the pane a prompt is waiting in, or the
   * panel of whatever is stuck ({@link attentionTarget}). Carried as the item
   * rather than as a route so the palette cannot drift from the strip — they
   * ask the same function the same question.
   */
  | { kind: "attention"; item: AttentionItem }
  | { kind: "page"; path: string }

export interface PaletteEntry {
  /**
   * cmdk's item value: what the row is *named*, and what tells two rows apart.
   * The shortened id ends it, because two tasks can share a title and two rows
   * with the same value are one row as far as cmdk is concerned.
   *
   * The full id is a keyword rather than part of this: `score.ts` matches the
   * keywords literally but never fuzzily, and 26 characters of ulid in the
   * fuzzy-matched text answers to almost any query.
   */
  value: string
  /** What the row is called: its primary text, and what leads it. */
  label: string
  /**
   * Secondary text, to the right of the label and under it in weight: the id,
   * the branch, the role. Long ones are truncated in the middle (see
   * `./detail`), so an id or a branch may show without its head.
   */
  detail?: string
  /** Searchable, literally, without being part of the row's name. */
  keywords: string[]
  target: PaletteTarget
}

interface PaletteSource {
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
      value: `${goal.title} ${shortId(goal.id)}`,
      label: goal.title,
      detail: shortId(goal.id),
      keywords: [goal.id, goal.status],
      target: { kind: "goal", goalId: goal.id },
    })),

    tasks: (tasks ?? []).map((task) => ({
      // The branch is what a task is called outside Ariadne, so it names the
      // row as much as the title does — searched for like the title, shown
      // beside it as the secondary text.
      value: `${task.title} ${task.branch}`,
      label: task.title,
      detail: task.branch,
      keywords: [task.id, task.status, goalTitles.get(task.goal_id) ?? ""],
      target: { kind: "task", taskId: task.id },
    })),

    // A session has no name of its own: it is "the reviewer of <task>", and
    // that is also how somebody looking for one describes it.
    sessions: (sessions ?? []).map((session) => {
      const of =
        (session.task_id ? taskTitles.get(session.task_id) : undefined) ??
        goalTitles.get(session.goal_id)
      return {
        value: `${ROLE_LABELS[session.role]} ${of ?? ""} ${shortId(session.id)}`,
        label: of ? `${ROLE_LABELS[session.role]} · ${of}` : ROLE_LABELS[session.role],
        detail: shortId(session.id),
        keywords: [session.id, session.status, session.tmux_session, session.agent_kind],
        target: {
          kind: "session",
          sessionId: session.id,
          goalId: session.goal_id,
          taskId: session.task_id ?? null,
        },
      }
    }),

    profiles: (profiles ?? []).map((profile) => ({
      value: `${profile.name} ${shortId(profile.id)}`,
      label: profile.name,
      detail: roleLabel(profile.role),
      keywords: [profile.id, profile.role, profile.model ?? "", profile.effort ?? ""],
      // The one entity with no panel of its own: the screen expands its row,
      // so the pick is carried there rather than dropped at `/profiles`.
      target: { kind: "page", path: paths.profile(profile.id) },
    })),
  }
}

/**
 * The route a picked entry navigates to, against the screen the palette was
 * opened over — which is what makes a task stack on the goal that is already
 * open rather than replacing it.
 *
 * The screen is its `pathname` as well as its params, because a task panel does
 * not open on every screen: the sessions one reads `?task=` as a filter of its
 * own, and {@link taskPanelFrom} sends the two targets built on it to the board
 * instead. A pick has to open what its row names.
 *
 * A session is shown inside the panel of the task it ran; a planner session
 * belongs to no task, and the goal panel only exists on the board, so that one
 * leaves the current screen for it.
 */
export function paletteTargetTo(
  target: PaletteTarget,
  search: URLSearchParams,
  pathname: string,
): string | { pathname?: string; search: string } {
  switch (target.kind) {
    case "goal":
      return paths.goal(target.goalId)
    case "task":
      return taskPanelFrom(pathname, search, target.taskId)
    case "attention":
      return attentionTarget(target.item, search, pathname)
    case "session":
      return target.taskId
        ? taskSessionPanelFrom(pathname, search, target.taskId, target.sessionId)
        : paths.goalSession(target.goalId, target.sessionId)
    case "page":
      return target.path
  }
}

/**
 * What is stuck, as palette rows: the same items the attention strip lists,
 * opening the same panels it opens.
 *
 * It is the answer to the first question anybody has, so the palette offers it
 * before anything has been typed — and it is worth the duplication with the
 * `Tasks` and `Sessions` groups precisely because those are alphabets of
 * everything, where this is the short list of what is asking for a person.
 *
 * The rows say *why* they are here rather than what they are named after: the
 * reason is the only thing that tells two otherwise identical rows apart, and
 * it is what decides whether the row is worth opening.
 */
export function attentionEntries(items: AttentionItem[]): PaletteEntry[] {
  return items.map((item) => ({
    // The shortened id ends it for the reason every other row's does: two
    // tasks can share a title, and two rows with the same value are one row to
    // cmdk — which here would silently drop half the list.
    value: `${attentionSubject(item)} ${reasonLabel(item)} ${shortId(item.id)}`,
    label: attentionSubject(item),
    detail: reasonLabel(item),
    keywords: [
      item.id,
      item.taskId ?? "",
      item.session?.id ?? "",
      item.taskReason ?? "",
      item.sessionReason ?? "",
      item.session ? ROLE_LABELS[item.session.role] : "",
      item.task?.branch ?? "",
      item.goal?.title ?? "",
    ],
    target: { kind: "attention", item },
  }))
}

/**
 * Why the row is on the list, in the badge's own word.
 *
 * The strip has room for the badges themselves and for
 * {@link import("@/features/goals/attention").attentionDetail}'s sentence under
 * them; a palette row has a column the width of a branch name, so it takes the
 * label and leaves the explanation to the panel the row opens. The session's
 * reason leads where there is one, which is the order the strip's badges read
 * in.
 */
function reasonLabel(item: AttentionItem): string {
  if (item.sessionReason) return SESSION_ATTENTION_META[item.sessionReason].label
  if (item.taskReason === "stalled") return STALLED_META.label
  return item.task ? TASK_STATUS_META[item.task.status].label : "Failed"
}
