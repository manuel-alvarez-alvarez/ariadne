/**
 * One agent session laid out in full: what it is, what it is saying, and
 * what it reported. No page chrome, so it renders the same inside the goal
 * panel and the task panel.
 *
 * What it is comes first and stays there — a compact block of facts, the same
 * shape as the task panel's — because it is what identifies the session, and
 * it is short. What it is *doing* is the long half, and the two halves of that
 * (the console and the reported events) are two answers to the same question:
 * they share the space as tabs rather than stacking into a page nobody reaches
 * the bottom of. The console is the tab that is open by default, since it is
 * why one opens a session at all.
 *
 * Switching tabs unmounts the console, which closes its socket. That is the
 * same trade `task-sessions.tsx` already takes for the selection itself:
 * every connection draws the newest page of the transcript afresh, so coming back costs a
 * reconnect and shows the same thing, where keeping it mounted would hold a
 * console open for nobody.
 *
 * The tab lives in the URL (`?tab=`), the way the goal and task panels keep
 * theirs: a reload stays on the tab the user was reading, and a link can point
 * at one agent's reported activity rather than at its console. It is the same
 * param those panels use — while one of them is drilled into a session it is
 * showing this view and nothing else, and coming back out sets `?tab=sessions`
 * again — so a value that is not one of these two simply reads as the default.
 *
 * The metadata comes from the query cache, which the event dispatcher keeps
 * current — a session going idle or being killed elsewhere updates this view
 * without a refetch. The console is the exception: it is a terminal, not
 * cacheable state, and owns its own connection (see `session-terminal.tsx`).
 *
 * The facts are dense, like the goal and task panels' own, and a chevron
 * folds them to the one line that still says what the console beneath them
 * is running and what it has spent — open by default, in local state nothing
 * outlives this mount. The stamps that say when (`Started`, `Last activity`)
 * sit on their own meta line under the heading instead of taking a fact's
 * width, the way the goal panel's own stamps do.
 *
 * This view is a flex column of a height its parent gives it: the tabs block
 * grows to fill whatever is left under the facts, and the console tab's
 * content grows inside *that*, so the terminal can ask for `h-full` and get a
 * real number rather than its own fallback height. The console takes the
 * space the pane leaves, even when the pane is short.
 */

import { useQuery } from "@tanstack/react-query"
import { ChevronDownIcon, ChevronRightIcon } from "lucide-react"
import { type ReactNode, useState } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { SessionDto } from "@/api"
import { CopyableId, CopyableIdMenu } from "@/components/copyable-id"
import { Fact, FactList } from "@/components/fact-list"
import { PanelHeader } from "@/components/panel-header"
import { TokenFigure } from "@/components/token-figure"
import { Button } from "@/components/ui/button"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When } from "@/components/when"
// Both read at the module that owns them rather than through their feature's
// barrel: `@/features/tasks` re-exports the task panel, whose sessions tab
// leads back here, and the round trip is an import cycle.
import { goalQueryOptions } from "@/features/goals/queries"
import { SeatSummary } from "@/features/models/agent-summary"
import { ModelPin } from "@/features/models/model-pin"
import { PullRequestLink } from "@/features/pull-requests/pull-request-link"
import { taskQueryOptions } from "@/features/tasks/queries"
import { sessionCopyEntries } from "@/lib/clipboard"
import { formatTokens } from "@/lib/format"
import { paths, usePanelSessionTo, useTaskPanelTo, useTerminalFocusRequest } from "@/routes/paths"

import { SessionActions } from "./session-actions"
import { SessionActivity } from "./session-activity"
import { SessionBlockedBanner } from "./session-blocked-banner"
import {
  SessionAttentionBadge,
  SessionStatusBadge,
  sessionHeading,
  shownAttention,
} from "./session-display"
import { SessionTerminal } from "./session-terminal"

/**
 * The session panel's own header: the breadcrumb back to whichever goal or
 * task it is drilled into, its heading, its actions, and the dense line of
 * its status, id and when it last moved — the one piece of a session's view
 * that is not shared between its three homes (the standalone session panel,
 * and the goal's and the task's own drill-downs), since each wraps a
 * different breadcrumb — or none — around the same content.
 */
export function SessionPanelHeader({
  session,
  breadcrumb,
  onResumed,
  onSwitched,
}: {
  session: SessionDto
  /** The goal or task this session is shown inside; omitted for the standalone panel. */
  breadcrumb?: { label: string; onClick: () => void }
  /** Where to go once a resume hands the session back; see {@link SessionActions}. */
  onResumed?: (session: SessionDto) => void
  /** Where to go once a switch creates a successor session. */
  onSwitched?: (session: SessionDto) => void
}) {
  const goal = useQuery({
    ...goalQueryOptions(session.goal_id ?? ""),
    enabled: Boolean(session.goal_id),
  })
  const attention = shownAttention(session)
  return (
    <PanelHeader
      breadcrumb={breadcrumb}
      title={sessionHeading(session)}
      actions={
        <SessionActions
          session={session}
          onResumed={onResumed}
          onSwitched={onSwitched}
          goalCancelled={goal.data?.status === "cancelled"}
        />
      }
      status={
        <>
          <SessionStatusBadge status={session.status} />
          {/* Next to the status rather than instead of it: the two are
              orthogonal — an agent blocked on a permission prompt is still
              running — and the pair is what says what to do about it. */}
          {attention ? <SessionAttentionBadge attention={attention} /> : null}
        </>
      }
      id={
        <CopyableIdMenu
          value={session.id}
          label="session id"
          entries={sessionCopyEntries(session.id)}
        />
      }
      stamps={
        <>
          <span>started</span>
          <When at={session.created_at} label="started" />
          <span aria-hidden="true">·</span>
          <span>{session.ended_at ? "ended" : "last activity"}</span>
          <When
            at={session.ended_at ?? session.last_activity_at}
            label={session.ended_at ? "ended" : "last activity"}
          />
        </>
      }
    />
  )
}

/**
 * The two halves of what a session is doing; the console is what is opened
 * for. Its tab is still `terminal` on the wire, so links made before the
 * console keep working — see `paths.ts`'s `sessionTerminalFrom`.
 */
const TABS = ["terminal", "activity"] as const
type Tab = (typeof TABS)[number]

export function SessionDetailView({
  session,
  context,
}: {
  session: SessionDto
  /**
   * What the view is embedded in. The link back to that goal or task is
   * dropped, since inside its own panel it would only point at itself.
   */
  context?: "goal" | "task"
}) {
  const goal = useQuery({
    ...goalQueryOptions(session.goal_id ?? ""),
    enabled: Boolean(session.goal_id),
  })
  const task = useQuery({
    ...taskQueryOptions(session.task_id ?? ""),
    enabled: Boolean(session.task_id),
  })
  const taskTo = useTaskPanelTo(session.task_id ?? "")
  const switchedFromTo = usePanelSessionTo(session.switched_from ?? "")
  const [search, setSearch] = useSearchParams()
  const tab = TABS.find((value) => value === search.get("tab")) ?? "terminal"
  // Set when the panel was opened by a row that said this agent is blocked on
  // a prompt: what it is waiting for is an answer, so the console takes the
  // keyboard rather than waiting to be clicked. Read once, on arrival.
  const focusTerminal = useTerminalFocusRequest()
  // Open by default — a session is opened to be read — and never remembered
  // past this mount: a fold is a glance away from being undone, not a
  // setting.
  const [factsOpen, setFactsOpen] = useState(true)
  const hasContext =
    session.context_used !== null &&
    session.context_used !== undefined &&
    session.context_size !== null &&
    session.context_size !== undefined

  // Replaces rather than pushes: which half of a session is on screen is not a
  // step of its own, and Back should leave the session, not walk its tabs.
  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      {/* Under the header rather than beside the badge: what to do about a
          blocked agent is a sentence, and the console it is about is below. */}
      <SessionBlockedBanner session={session} />

      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="icon-xs"
            aria-expanded={factsOpen}
            aria-label={`${factsOpen ? "Collapse" : "Expand"} session facts`}
            onClick={() => setFactsOpen((open) => !open)}
          >
            {factsOpen ? <ChevronDownIcon /> : <ChevronRightIcon />}
          </Button>
          {/* Folded, the facts are the one line that still says what the
              console below is running and what it has spent. */}
          {factsOpen ? null : (
            <div className="flex min-w-0 items-center gap-3 text-xs">
              <ModelPin model={session.model} effort={session.effort} mode="line" />
              <TokenFigure usage={session.usage} />
            </div>
          )}
        </div>
        {factsOpen ? (
          <FactList dense>
            {context === "goal" || !session.goal_id ? null : (
              <Fact label="Goal">
                <Link to={paths.goal(session.goal_id)} className="block truncate hover:underline">
                  {goal.data?.title ?? <Mono>{session.goal_id}</Mono>}
                </Link>
              </Fact>
            )}
            {context === "task" || !session.task_id ? null : (
              <Fact label="Task">
                {/* Replaces rather than pushes exactly when this session is a
                    goal's own drill-down (`taskTo.replace`): the task panel
                    takes the goal's place in the pane rather than stacking on
                    it, same as the task panel's own breadcrumb. */}
                <Link
                  to={taskTo}
                  replace={taskTo.replace}
                  className="block truncate hover:underline"
                >
                  {task.data?.title ?? <Mono>{session.task_id}</Mono>}
                </Link>
              </Fact>
            )}
            {/* A pull request session works for no goal and no task: the
                request it watches is what it is about, linked to the forge. */}
            {session.pull_request_id ? (
              <Fact label="Pull request">
                <PullRequestLink id={session.pull_request_id} title={session.title} />
              </Fact>
            ) : null}
            {/* One fact, not two: what the agent runs on is the tail of this line
                (`claude-agent-acp:claude-opus-5`), and a Model row under it repeated
                that tail with the agent half taken off. The session's own snapshot,
                not the profile's current fields — the profile may have been edited
                since this agent was launched. A loose session — one resumed from
                an outside conversation — names its agent the same way, having no
                seat to lead with. */}
            <Fact label="Agent">
              {session.seat ? (
                <SeatSummary seat={session.seat} model={session.model} effort={session.effort} />
              ) : (
                <ModelPin model={session.model} effort={session.effort} mode="line" />
              )}
            </Fact>
            {session.switched_from ? (
              <Fact label="Continues">
                <Link to={switchedFromTo} replace className="block truncate hover:underline">
                  <Mono>{session.switched_from}</Mono>
                </Link>
              </Fact>
            ) : null}
            {session.worktree_path ? (
              <Fact label="Directory">
                <CopyableId value={session.worktree_path} label="working directory" />
              </Fact>
            ) : null}
            <Fact label="Agent session id">
              {session.internal_session_id ? (
                <CopyableId value={session.internal_session_id} label="agent session id" />
              ) : (
                <Dash />
              )}
            </Fact>
            {/* Every transcript this agent reported under, summed — so a session
                resumed into the same agent conversation reads as one figure. Zeros
                until it reports anything, which is a number and not a blank: an
                agent that has spent nothing is what a session just spawned is. */}
            <Fact label="Tokens">
              <TokenFigure usage={session.usage} />
            </Fact>
            {hasContext ? (
              <Fact label="Context">
                <ContextMeter used={session.context_used ?? 0} size={session.context_size ?? 0} />
              </Fact>
            ) : null}
            {session.attention_reason ? (
              <Fact label="Needs attention since">
                <When at={session.attention_since} label="since" />
              </Fact>
            ) : null}
          </FactList>
        ) : null}
      </div>

      {/* `min-h-0 flex-1` so this block, not the view's own height, is what
          grows to fill the panel — and the console tab's content below
          inherits the same contract, which is how it ends up with a real
          height to hand `SessionTerminal` instead of its own fallback. */}
      <Tabs
        value={tab}
        onValueChange={(value) => setTab(value as Tab)}
        className="flex min-h-0 flex-1 flex-col"
      >
        <TabsList>
          <TabsTrigger value="terminal">Console</TabsTrigger>
          <TabsTrigger value="activity">Agent activity</TabsTrigger>
        </TabsList>
        <TabsContent value="terminal" className="min-h-0 flex flex-1 flex-col pt-3">
          <SessionTerminal
            sessionId={session.id}
            status={session.status}
            autoFocus={focusTerminal}
            className="h-full min-h-0"
          />
        </TabsContent>
        <TabsContent value="activity" className="pt-3">
          <SessionActivity sessionId={session.id} />
        </TabsContent>
      </Tabs>
    </div>
  )
}

/**
 * The plain mono face, for ids that stand in for a name inside a link: the
 * click there belongs to the link, so those ids are not copy targets — the
 * session's own id in the header above is.
 */
function Mono({ children }: { children: ReactNode }) {
  return <code className="font-mono text-xs">{children}</code>
}

function Dash() {
  return <span className="text-muted-foreground">—</span>
}

/**
 * The reported context window as a bar, 4rem wide, beside the pair of
 * figures it is a bar *of* — decorative rather than its own fact, since the
 * text beside it already says the same thing a screen reader can read.
 */
function ContextMeter({ used, size }: { used: number; size: number }) {
  const fraction = size > 0 ? Math.min(1, Math.max(0, used / size)) : 0
  return (
    <span className="flex items-center gap-2">
      <span
        aria-hidden="true"
        className="h-1.5 w-16 shrink-0 overflow-hidden rounded-full bg-muted"
      >
        <span
          className="block h-full rounded-full bg-foreground"
          style={{ width: `${fraction * 100}%` }}
        />
      </span>
      <span className="tabular-nums">
        {formatTokens(used)} / {formatTokens(size)}
      </span>
    </span>
  )
}
