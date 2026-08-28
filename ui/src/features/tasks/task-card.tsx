/**
 * One task, as it appears on the board and in any other list of tasks.
 *
 * What earns space here is what tells you whether a task needs attention:
 * whether one of its agents is waiting on a person, which round of review it
 * is on, whether its agent went idle, and how many other tasks it is waiting
 * for — plus the branch, which is the one string an
 * engineer actually wants off the card and into a terminal. It sits outside
 * the link on purpose: a copy button nested in an anchor is neither valid nor
 * clickable without hijacking the navigation.
 *
 * Everything explanatory is a real `Tooltip`. `title=` attributes were the
 * cheaper way to say the same things, and they are unreachable by keyboard —
 * which is only true of the tooltips because every trigger takes focus, spans
 * and `<time>`s included. That is the primitive's doing, not each card's; see
 * `components/ui/tooltip.tsx`.
 */

import { useQuery } from "@tanstack/react-query"
import { CpuIcon, GitBranchIcon, LayersIcon, TriangleAlertIcon } from "lucide-react"
import { Link } from "react-router-dom"

import type { TaskDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { StatusBadge } from "@/components/status-badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When } from "@/components/when"
import { agentKindLabel, modelLabel } from "@/features/profiles/profile-labels"
import { isPinOverride } from "@/features/profiles/profile-summary"
import { profilesQueryOptions } from "@/features/profiles/queries"
import {
  SESSION_ATTENTION_META,
  type SessionAttention,
  SessionAttentionBadge,
} from "@/features/sessions/session-display"
import { cn, plural } from "@/lib/format"
import { useTaskPanelTo } from "@/routes/paths"
import { STALLED_META } from "./stalled"
import { primaryStatus, subStatus, TASK_STATUS_META } from "./status"

/** What the card says about a task its goal's plan is still holding back. */
const AWAITING_PLAN = {
  label: "Awaiting plan",
  hint: "Its goal is still being planned: nothing starts until the planner finalizes the plan.",
  badge: "bg-status-warn-soft text-status-warn-fg",
}

export function TaskCard({
  task,
  showStatus = false,
  attention,
  awaitingPlan = false,
}: {
  task: TaskDto
  showStatus?: boolean
  /**
   * Why one of this task's sessions wants a person, when one does — the flag
   * the daemon raised, handed down by whoever is holding the sessions list
   * (`useBoardAttention`) rather than fetched per card.
   *
   * A card is where the work is, and until this was on it the only way to see
   * that a task was blocked on a permission prompt was to read the strip above
   * the board and match the titles up.
   */
  attention?: SessionAttention | null
  /**
   * Whether this task is held by its goal's plan rather than by anything of
   * its own — what the board says about every card of a `planning` goal, which
   * is why it is the lane's word and not the card's.
   */
  awaitingPlan?: boolean
}) {
  const status = TASK_STATUS_META[primaryStatus(task.status)]
  const sub = subStatus(task.status)
  const terminal = task.status === "cancelled"
  const to = useTaskPanelTo(task.id)

  return (
    <div
      className={cn(
        "rounded-lg border bg-card transition-colors hover:border-foreground/20 hover:bg-muted/50",
        // An agent waiting on a person outranks a stall the same agent is
        // being counted as: both are drawn as a coloured outline, and the one
        // that says what to do about it is this.
        attention ? SESSION_ATTENTION_META[attention].border : task.stalled && STALLED_META.border,
      )}
    >
      <Link
        to={to}
        className="block rounded-lg px-2.5 pt-2.5 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
      >
        <p
          className={cn(
            "line-clamp-2 text-sm leading-snug font-medium",
            terminal && "text-muted-foreground line-through decoration-muted-foreground/40",
          )}
        >
          {task.title}
        </p>

        <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          {/* First in the row, ahead of the status: it is the one thing on the
              card that is addressed to the reader. Same pill as the attention
              strip and the sessions table, down to the wording. */}
          {attention && <SessionAttentionBadge size="sm" attention={attention} />}
          {showStatus && <StatusBadge size="sm" label={status.label} tone={status.badge} />}
          {sub && (
            <Tooltip>
              <TooltipTrigger render={<span className="flex" />}>
                <StatusBadge size="sm" label={sub.label} tone={sub.badge} />
              </TooltipTrigger>
              <TooltipContent>{sub.hint}</TooltipContent>
            </Tooltip>
          )}
          {/* After the status, because it refines it: a pending or ready task
              of a plan still being written is not waiting on an engineer, it
              is waiting on the planner. */}
          {awaitingPlan && (
            <Tooltip>
              <TooltipTrigger render={<span className="flex" />}>
                <StatusBadge size="sm" label={AWAITING_PLAN.label} tone={AWAITING_PLAN.badge} />
              </TooltipTrigger>
              <TooltipContent>{AWAITING_PLAN.hint}</TooltipContent>
            </Tooltip>
          )}
          {task.review_round > 0 && (
            <Tooltip>
              <TooltipTrigger render={<span className="font-mono" />}>
                R{task.review_round}
              </TooltipTrigger>
              <TooltipContent>Review round {task.review_round}</TooltipContent>
            </Tooltip>
          )}
          {task.depends_on.length > 0 && (
            <Tooltip>
              <TooltipTrigger render={<span className="flex items-center gap-1" />}>
                <LayersIcon className="size-3" />
                {task.depends_on.length}
              </TooltipTrigger>
              <TooltipContent>
                Waits for {plural(task.depends_on.length, "task")} to merge
              </TooltipContent>
            </Tooltip>
          )}
          {task.stalled && (
            <Tooltip>
              <TooltipTrigger
                render={
                  <span className={cn("flex items-center gap-1 font-medium", STALLED_META.text)} />
                }
              >
                <TriangleAlertIcon className="size-3" />
                {STALLED_META.label}
              </TooltipTrigger>
              <TooltipContent>{STALLED_META.hint}</TooltipContent>
            </Tooltip>
          )}
          <When at={task.updated_at} label="updated" className="ml-auto" />
        </div>
      </Link>

      {/* Stacked rather than wrapped: each pill is `w-fit max-w-full`, and as
          flex items on one row they would squeeze each other's middle-truncated
          text instead of taking the width they need. */}
      <div className="flex flex-col items-start gap-1.5 px-2.5 pt-1.5 pb-2.5">
        <span className="flex w-fit max-w-full items-center gap-1 rounded-md bg-muted/60 px-1.5 py-0.5 text-xs text-muted-foreground">
          <GitBranchIcon className="size-3 shrink-0" />
          {/* Middle-truncated: the card is narrow, and what an engineer looks
              for is the slug at the end rather than the ULID it hangs off. */}
          <CopyableId value={task.branch} label="branch" truncate="middle" />
        </span>
        <EnginePin task={task} />
      </div>
    </div>
  )
}

/**
 * What the engineer of this task runs on, when that is not what its profile
 * says — a model chosen for the task, or a profile edited away from the pin
 * since.
 *
 * Only then: the pin is on every task, and a board repeating the same profile
 * default on every card would say nothing while costing every card a line.
 * What the reader cannot know without being told is the card that runs on
 * something else, and the task panel's facts spell the pin out either way.
 */
function EnginePin({ task }: { task: TaskDto }) {
  const profiles = useQuery(profilesQueryOptions())
  const profile = profiles.data?.find((item) => item.id === task.engineer_profile_id)
  if (!isPinOverride(profile, task)) return null

  const pin = `${agentKindLabel(task.agent_kind)} · ${modelLabel(task.model)}`
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <span className="flex w-fit min-w-0 items-center gap-1 rounded-md bg-muted/60 px-1.5 py-0.5 text-xs text-muted-foreground" />
        }
      >
        <CpuIcon className="size-3 shrink-0" />
        <span className="truncate font-mono">{pin}</span>
      </TooltipTrigger>
      <TooltipContent>{`${pin} (overrides ${profile?.name})`}</TooltipContent>
    </Tooltip>
  )
}
