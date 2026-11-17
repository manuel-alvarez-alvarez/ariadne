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

import {
  type GoalDto,
  type Host,
  type HostRow,
  LOCAL_HOST,
  type RepositoryDto,
  type SessionDto,
  type SkillDto,
  type TaskDto,
  type WorkflowDto,
} from "@/api"
import {
  type AttentionItem,
  type AttentionNavTarget,
  agentLabel,
  attentionSubject,
  attentionTarget,
} from "@/features/goals/attention"
import { SESSION_ATTENTION_META, seatLabel } from "@/features/sessions/session-display"
import { STALLED_META, TASK_STATUS_META } from "@/features/tasks"
import { sessionColumn } from "@/features/tasks/steps"
import { folderName, shortId } from "@/lib/format"
import { paths, taskPanelFrom, taskSessionPanelFrom } from "@/routes/paths"

/** Where a palette entry goes when it is picked. */
type PaletteTarget =
  | { kind: "goal"; host: Host; goalId: string }
  | { kind: "task"; host: Host; taskId: string }
  | { kind: "session"; host: Host; sessionId: string; goalId: string; taskId: string | null }
  /**
   * A row of the attention list, which decides where it goes itself: the
   * thread a question was asked in, the console a prompt is waiting in, or the
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
   * the branch, the seat. Long ones are truncated in the middle (see
   * `./detail`), so an id or a branch may show without its head.
   */
  detail?: string
  /** The host a goal, task or session row lives on; the row names it unless it is `local`. */
  host?: Host
  /** Searchable, literally, without being part of the row's name. */
  keywords: string[]
  target: PaletteTarget
}

interface PaletteSource {
  goals: HostRow<GoalDto>[] | undefined
  tasks: HostRow<TaskDto>[] | undefined
  sessions: HostRow<SessionDto>[] | undefined
  skills: SkillDto[] | undefined
  repositories: RepositoryDto[] | undefined
  workflows: WorkflowDto[] | undefined
}

/** One group of rows per entity, in the order the palette lists them. */
export interface PaletteEntries {
  goals: PaletteEntry[]
  tasks: PaletteEntry[]
  sessions: PaletteEntry[]
  skills: PaletteEntry[]
  repositories: PaletteEntry[]
  workflows: PaletteEntry[]
}

/** Two rows with one title on two hosts are two rows: the host ends the name. */
function hostSuffix(host: Host): string {
  return host === LOCAL_HOST ? "" : ` @${host}`
}

/** What a search by host name matches a named host's row by. */
function hostWords(host: Host): string[] {
  return host === LOCAL_HOST ? [] : [host]
}

export function buildPaletteEntries({
  goals,
  tasks,
  sessions,
  skills,
  repositories,
  workflows,
}: PaletteSource): PaletteEntries {
  // Ids are one daemon's own, so a goal or a task is looked up under its host.
  const goalTitles = new Map((goals ?? []).map((goal) => [`${goal.host}:${goal.id}`, goal.title]))
  const taskTitles = new Map((tasks ?? []).map((task) => [`${task.host}:${task.id}`, task.title]))
  const tasksById = new Map((tasks ?? []).map((task) => [`${task.host}:${task.id}`, task]))
  const goalsById = new Map((goals ?? []).map((goal) => [`${goal.host}:${goal.id}`, goal]))

  return {
    goals: (goals ?? []).map((goal) => ({
      value: `${goal.title} ${shortId(goal.id)}${hostSuffix(goal.host)}`,
      label: goal.title,
      detail: shortId(goal.id),
      host: goal.host,
      keywords: [goal.id, goal.status, ...hostWords(goal.host)],
      target: { kind: "goal", host: goal.host, goalId: goal.id },
    })),

    tasks: (tasks ?? []).map((task) => ({
      // The branch is what a task is called outside Ariadne, so it names the
      // row as much as the title does — searched for like the title, shown
      // beside it as the secondary text.
      value: `${task.title} ${task.branch}${hostSuffix(task.host)}`,
      label: task.title,
      detail: task.branch,
      host: task.host,
      keywords: [
        task.id,
        task.status,
        goalTitles.get(`${task.host}:${task.goal_id}`) ?? "",
        ...hostWords(task.host),
      ],
      target: { kind: "task", host: task.host, taskId: task.id },
    })),

    // A session has no name of its own: it is "the reviewer of <task>", and
    // that is also how somebody looking for one describes it.
    sessions: (sessions ?? []).map((session) => {
      const of =
        (session.task_id ? taskTitles.get(`${session.host}:${session.task_id}`) : undefined) ??
        goalTitles.get(`${session.host}:${session.goal_id ?? ""}`)
      // A step agent is named by the workflow column it staffs.
      const seat =
        sessionColumn(
          session,
          tasksById.get(`${session.host}:${session.task_id ?? ""}`),
          goalsById.get(`${session.host}:${session.goal_id ?? ""}`),
        ) ?? seatLabel(session.seat)
      return {
        value: `${seat} ${of ?? ""} ${shortId(session.id)}${hostSuffix(session.host)}`,
        label: of ? `${seat} · ${of}` : seat,
        detail: shortId(session.id),
        host: session.host,
        keywords: [session.id, session.status, session.model, ...hostWords(session.host)],
        target: {
          kind: "session",
          host: session.host,
          sessionId: session.id,
          goalId: session.goal_id ?? "",
          taskId: session.task_id ?? null,
        },
      }
    }),

    skills: (skills ?? []).map((skill) => ({
      value: skill.name,
      label: skill.name,
      detail: skill.summary,
      keywords: [skill.name, skill.builtin ? "shipped" : "yours"],
      // The one entity with no panel of its own: the screen opens on it,
      // so the pick is carried there rather than dropped at `/skills`.
      target: { kind: "page", path: paths.skill(skill.name) },
    })),

    workflows: (workflows ?? []).map((workflow) => ({
      value: workflow.name,
      label: workflow.name,
      detail: workflow.steps.map((step) => step.title).join(" · "),
      keywords: [workflow.name, workflow.builtin ? "shipped" : "yours"],
      target: { kind: "page", path: paths.workflow(workflow.name) },
    })),

    // A repository has no panel either, and no screen of its own to open on
    // one: a pick lands on the list that holds every registered checkout.
    repositories: (repositories ?? []).map((repository) => ({
      value: `${folderName(repository.path)} ${shortId(repository.id)}`,
      label: folderName(repository.path),
      detail: shortId(repository.id),
      keywords: [repository.id, repository.path],
      target: { kind: "page", path: paths.repositories() },
    })),
  }
}

/**
 * The route a picked entry navigates to, against the screen the palette was
 * opened over — which is what makes a task open on whatever screen is
 * showing, replacing a goal already open on it rather than sitting over it.
 *
 * The screen is its `pathname` as well as its params, because a task panel does
 * not open on every screen: the sessions one reads `?task=` as a filter of its
 * own, and {@link taskPanelFrom} sends the two targets built on it to the board
 * instead. A pick has to open what its row names.
 *
 * A session is shown inside the panel of the task it ran; an orchestrator session
 * belongs to no task, and the goal panel only exists on the board, so that one
 * leaves the current screen for it.
 *
 * A task target's `replace` (set by `taskPanelTo` in `routes/paths.ts`) rides
 * along on every kind built on it — `task`, `attention`, and `session` inside
 * a task — so `go` only has to read it off what comes back, never work it
 * out again.
 */
export function paletteTargetTo(
  target: PaletteTarget,
  search: URLSearchParams,
  pathname: string,
): string | AttentionNavTarget {
  switch (target.kind) {
    case "goal":
      return paths.goal(target.goalId, target.host)
    case "task":
      return taskPanelFrom(pathname, search, target.taskId, target.host)
    case "attention":
      return attentionTarget(target.item, search, pathname)
    case "session":
      return target.taskId
        ? taskSessionPanelFrom(pathname, search, target.taskId, target.sessionId, target.host)
        : paths.goalSession(target.goalId, target.sessionId, target.host)
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
    value: `${attentionSubject(item)} ${reasonLabel(item)} ${shortId(item.id)}${hostSuffix(item.host)}`,
    label: attentionSubject(item),
    detail: reasonLabel(item),
    host: item.host,
    keywords: [
      item.id,
      item.taskId ?? "",
      item.session?.id ?? "",
      item.taskReason ?? "",
      item.sessionReason ?? "",
      agentLabel(item),
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
