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
 * The card says each of those things **once**. A badge that repeats a label
 * already on the card is dropped rather than drawn twice (see
 * {@link repeatsCard}), which is what a stalled task saying "Stalled" beside
 * "⚠ Stalled" was.
 *
 * **The whole card is one tab stop.** Everything explanatory is a real
 * `Tooltip` for a pointer, and not one of them is focusable. They used to be:
 * the tooltip primitive makes every trigger focusable
 * (`components/ui/tooltip.tsx`), so a card was some seven stops of spans — six
 * of them nested inside an anchor, which is an interactive node inside an
 * interactive one — and a board of thirty cards was two hundred stops of
 * hints. Every one of those hints is collected into one hidden line instead
 * ({@link cardHints}) and hung off the link with `aria-describedby`: nothing
 * that was only in a tooltip has stopped being reachable, it is read where
 * the card is landed on rather than seven Tabs into it.
 *
 * That includes the branch's copy button, which is the one *control* here
 * rather than a hint. It keeps its name and its click; what it gives up is
 * being the card's second stop, and the keyboard route to the branch is the
 * same control in the task panel this card's link opens.
 */

import { useQuery } from "@tanstack/react-query"
import { CpuIcon, GitBranchIcon, LayersIcon, TriangleAlertIcon } from "lucide-react"
import { useId } from "react"
import { Link } from "react-router-dom"

import type { TaskDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { StatusBadge } from "@/components/status-badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When, whenHint } from "@/components/when"
import { modelRefLabel } from "@/features/profiles/model-ref"
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

/**
 * The card is one tab stop, so nothing drawn on it takes one of its own —
 * hints inside the link and controls beside it alike. Every hint is read off
 * the link's description instead ({@link cardHints}); the copy button keeps
 * its click and its name.
 */
const NOT_A_STOP = -1

export function TaskCard({
  task,
  showStatus = false,
  attention,
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
}) {
  const status = TASK_STATUS_META[primaryStatus(task.status)]
  const sub = subStatus(task.status)
  const terminal = task.status === "cancelled"
  const to = useTaskPanelTo(task.id)
  // The reason is still what outlines the card even where the badge for it is
  // dropped: it is the same fact, said once.
  const flagged = attention && !repeatsCard(attention, task, showStatus) ? attention : null
  const pin = useEnginePin(task)
  const hintsId = useId()
  const hints = cardHints(task, { status: showStatus ? status.hint : null, flagged, pin })

  return (
    <div
      className={cn(
        "rounded-lg border bg-card transition-colors hover:border-foreground/20 hover:bg-muted/50",
        cardBorder(task, attention),
      )}
    >
      <Link
        to={to}
        aria-describedby={hints.length > 0 ? hintsId : undefined}
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
          {flagged && (
            <SessionAttentionBadge size="sm" attention={flagged} hintTabIndex={NOT_A_STOP} />
          )}
          {showStatus && (
            <StatusBadge
              size="sm"
              label={status.label}
              tone={status.badge}
              hint={status.hint}
              hintTabIndex={NOT_A_STOP}
            />
          )}
          {sub && (
            <Tooltip>
              <TooltipTrigger tabIndex={NOT_A_STOP} render={<span className="flex" />}>
                <StatusBadge size="sm" label={sub.label} tone={sub.badge} />
              </TooltipTrigger>
              <TooltipContent>{sub.hint}</TooltipContent>
            </Tooltip>
          )}
          {task.review_round > 0 && (
            <Tooltip>
              <TooltipTrigger tabIndex={NOT_A_STOP} render={<span className="font-mono" />}>
                R{task.review_round}
              </TooltipTrigger>
              <TooltipContent>{reviewRoundHint(task)}</TooltipContent>
            </Tooltip>
          )}
          {task.depends_on.length > 0 && (
            <Tooltip>
              <TooltipTrigger
                tabIndex={NOT_A_STOP}
                render={<span className="flex items-center gap-1" />}
              >
                <LayersIcon className="size-3" />
                {task.depends_on.length}
              </TooltipTrigger>
              <TooltipContent>{dependencyHint(task)}</TooltipContent>
            </Tooltip>
          )}
          {task.stalled && (
            <Tooltip>
              <TooltipTrigger
                tabIndex={NOT_A_STOP}
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
          <When
            at={task.updated_at}
            label="updated"
            hintTabIndex={NOT_A_STOP}
            className="ml-auto"
          />
        </div>
      </Link>

      {/* Everything the link says only to a pointer, in one line for whoever
          is reading the link itself. Outside the anchor, so it describes the
          link rather than joining its name. */}
      {hints.length > 0 && (
        <p id={hintsId} className="sr-only">
          {hints.join(". ")}
        </p>
      )}

      {/* Stacked rather than wrapped: each pill is `w-fit max-w-full`, and as
          flex items on one row they would squeeze each other's middle-truncated
          text instead of taking the width they need. */}
      <div className="flex flex-col items-start gap-1.5 px-2.5 pt-1.5 pb-2.5">
        <span className="flex w-fit max-w-full items-center gap-1 rounded-md bg-muted/60 px-1.5 py-0.5 text-xs text-muted-foreground">
          <GitBranchIcon className="size-3 shrink-0" />
          {/* Middle-truncated: the card is narrow, and what an engineer looks
              for is the slug at the end rather than the ULID it hangs off. */}
          <CopyableId value={task.branch} label="branch" truncate="middle" tabIndex={NOT_A_STOP} />
        </span>
        {pin && (
          <Tooltip>
            <TooltipTrigger
              tabIndex={NOT_A_STOP}
              render={
                <span className="flex w-fit min-w-0 items-center gap-1 rounded-md bg-muted/60 px-1.5 py-0.5 text-xs text-muted-foreground" />
              }
            >
              <CpuIcon className="size-3 shrink-0" />
              <span className="truncate font-mono">{pin.label}</span>
            </TooltipTrigger>
            <TooltipContent>{pin.hint}</TooltipContent>
          </Tooltip>
        )}
      </div>
    </div>
  )
}

/**
 * The outline the card wears, when it wears one — one colour for however many
 * things are wrong with the task at once.
 *
 * In the order `taskAttentionReason` and `compareByAttention` rank them: an
 * agent waiting on a *person* leads, because it is the only one of the three
 * that says what to do about it; then the failure, which is where the task
 * actually is and which has to stand out in a Pending column full of tasks
 * that have simply not started; then the stall, which is a flag on top of
 * whatever status the task is parked in.
 */
function cardBorder(task: TaskDto, attention?: SessionAttention | null): string | undefined {
  if (attention) return SESSION_ATTENTION_META[attention].border
  return TASK_STATUS_META[task.status].border ?? (task.stalled ? STALLED_META.border : undefined)
}

/**
 * Whether the attention badge would only say again what the card already says.
 *
 * The daemon raises a session reason and the task carries a flag of its own,
 * and for a stall they are the same fact seen from two places — which the card
 * was drawing as `Stalled` beside `⚠ Stalled`. Compared by label rather than
 * by naming the one pair that collides today: two vocabularies that share a
 * word will share it again.
 */
function repeatsCard(attention: SessionAttention, task: TaskDto, showStatus: boolean): boolean {
  const said = new Set<string>()
  if (showStatus) said.add(TASK_STATUS_META[primaryStatus(task.status)].label)
  const sub = subStatus(task.status)
  if (sub) said.add(sub.label)
  if (task.stalled) said.add(STALLED_META.label)
  return said.has(SESSION_ATTENTION_META[attention].label)
}

/**
 * Every hint on the card, in reading order — the whole of what the tooltips
 * say, since none of them can be focused.
 *
 * All of them, not only the ones inside the link: the pin under it is on the
 * card too, and the link is the card's only stop, so this is the one place a
 * keyboard is told what "overrides Engineer" or the exact stamp behind "2
 * hours ago" was going to say. Anything a tooltip is the *only* home for
 * belongs here; the branch and the pin's own label do not, because they are
 * written on the card in plain text.
 */
function cardHints(
  task: TaskDto,
  {
    status,
    flagged,
    pin,
  }: { status: string | null; flagged: SessionAttention | null; pin: EnginePin | null },
): string[] {
  const hints: string[] = []
  if (flagged) hints.push(SESSION_ATTENTION_META[flagged].hint)
  if (status) hints.push(status)
  const sub = subStatus(task.status)
  if (sub) hints.push(sub.hint)
  if (task.review_round > 0) hints.push(reviewRoundHint(task))
  if (task.depends_on.length > 0) hints.push(dependencyHint(task))
  if (task.stalled) hints.push(STALLED_META.hint)
  hints.push(whenHint(task.updated_at, "updated"))
  if (pin) hints.push(pin.hint)
  return hints
}

function reviewRoundHint(task: TaskDto): string {
  return `Review round ${task.review_round}`
}

function dependencyHint(task: TaskDto): string {
  return `Waits for ${plural(task.depends_on.length, "task")} to merge`
}

/** What the pin says on the card, and what it says behind it. */
interface EnginePin {
  label: string
  hint: string
}

/**
 * What the engineer of this task runs on, when that is not what its profile
 * says — a model chosen for the task, or a profile edited away from the pin
 * since. `null` when it is the profile's own, which is most cards.
 *
 * Only then: the pin is on every task, and a board repeating the same profile
 * default on every card would say nothing while costing every card a line.
 * What the reader cannot know without being told is the card that runs on
 * something else, and the task panel's facts spell the pin out either way.
 *
 * A hook rather than the component it used to be, because the card has to put
 * the hint in its own description: a pill that cannot be focused cannot carry
 * its tooltip anywhere else.
 */
function useEnginePin(task: TaskDto): EnginePin | null {
  const profiles = useQuery(profilesQueryOptions())
  const profile = profiles.data?.find((item) => item.id === task.engineer_profile_id)
  if (!isPinOverride(profile, task.model)) return null

  const label = modelRefLabel(task.model)
  return { label, hint: `${label} (overrides ${profile?.name})` }
}
