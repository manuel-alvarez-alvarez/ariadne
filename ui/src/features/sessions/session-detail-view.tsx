/**
 * One agent session laid out in full: what it is, what it is printing, and
 * what it reported. No page chrome, so it renders the same inside the goal
 * panel and the task panel.
 *
 * What it is comes first and stays there — a compact block of facts, the same
 * shape as the task panel's — because it is what identifies the session, and
 * it is short. What it is *doing* is the long half, and the two halves of that
 * (the pane and the reported events) are two answers to the same question:
 * they share the space as tabs rather than stacking into a page nobody reaches
 * the bottom of. The terminal is the tab that is open by default, since it is
 * why one opens a session at all.
 *
 * Switching tabs unmounts the terminal, which drops its log stream. That is
 * the same trade `task-sessions.tsx` already takes for the selection itself:
 * every connection replays the whole pane from a snapshot, so coming back
 * costs a reconnect and shows the same thing, where keeping it mounted would
 * hold a stream open for a pane nobody is looking at.
 *
 * The tab lives in the URL (`?tab=`), the way the goal and task panels keep
 * theirs: a reload stays on the tab the user was reading, and a link can point
 * at one agent's reported activity rather than at its pane. It is the same
 * param those panels use — while one of them is drilled into a session it is
 * showing this view and nothing else, and coming back out sets `?tab=sessions`
 * again — so a value that is not one of these two simply reads as the default.
 *
 * The metadata comes from the query cache, which the event dispatcher keeps
 * current — a session going idle or being killed elsewhere updates this view
 * without a refetch. The terminal is the exception: it is a byte stream, not
 * cacheable state, and owns its own connection (see `log-stream.ts`).
 */

import { useQuery } from "@tanstack/react-query"
import type { ReactNode } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { SessionDto } from "@/api"
import { CopyableId, CopyableIdMenu } from "@/components/copyable-id"
import { Fact, FactList } from "@/components/fact-list"
import { TokenFigure } from "@/components/token-figure"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When } from "@/components/when"
import { formatModelRef } from "@/features/profiles/model-ref"
import { ProfileSummary } from "@/features/profiles/profile-summary"
import { sessionCopyEntries } from "@/lib/clipboard"
import { ROLE_LABELS } from "@/lib/format"
import { paths, useTaskPanelTo, useTerminalFocusRequest } from "@/routes/paths"

import { goalQueryOptions, taskQueryOptions } from "./queries"
import { SessionActions } from "./session-actions"
import { SessionActivity } from "./session-activity"
import { SessionBlockedBanner } from "./session-blocked-banner"
import { SessionAttentionBadge, SessionStatusBadge } from "./session-display"
import { SessionTerminal } from "./session-terminal"

/** The two halves of what a session is doing; the pane is what is opened for. */
const TABS = ["terminal", "activity"] as const
type Tab = (typeof TABS)[number]

export function SessionDetailView({
  session,
  context,
  onResumed,
}: {
  session: SessionDto
  /**
   * What the view is embedded in. The link back to that goal or task is
   * dropped, since inside its own panel it would only point at itself.
   */
  context?: "goal" | "task"
  /** Where to go once a resume hands the session back; see {@link SessionActions}. */
  onResumed?: (session: SessionDto) => void
}) {
  const goal = useQuery(goalQueryOptions(session.goal_id))
  const task = useQuery({
    ...taskQueryOptions(session.task_id ?? ""),
    enabled: Boolean(session.task_id),
  })
  const taskTo = useTaskPanelTo(session.task_id ?? "")
  const [search, setSearch] = useSearchParams()
  const tab = TABS.find((value) => value === search.get("tab")) ?? "terminal"
  // Set when the panel was opened by a row that said this agent is blocked on
  // a prompt: what it is waiting for is a keystroke, so the pane takes the
  // keyboard rather than waiting to be clicked. Read once, on arrival.
  const focusTerminal = useTerminalFocusRequest()

  // Replaces rather than pushes: which half of a session is on screen is not a
  // step of its own, and Back should leave the session, not walk its tabs.
  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <div className="space-y-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="font-heading text-xl font-semibold tracking-tight">
          {ROLE_LABELS[session.role]} session
        </h1>
        <SessionStatusBadge status={session.status} />
        {/* Next to the status rather than instead of it: the two are
            orthogonal — an agent blocked on a permission prompt is still
            running — and the pair is what says what to do about it. */}
        {session.attention_reason ? (
          <SessionAttentionBadge attention={session.attention_reason} />
        ) : null}
        <CopyableIdMenu
          value={session.id}
          label="session id"
          entries={sessionCopyEntries(session.id)}
          className="text-xs text-muted-foreground"
        />
        <div className="ml-auto">
          <SessionActions session={session} onResumed={onResumed} />
        </div>
      </header>

      {/* Under the header rather than beside the badge: what to do about a
          blocked agent is a sentence, and the pane it is about is below. */}
      <SessionBlockedBanner session={session} />

      <FactList>
        {context === "goal" ? null : (
          <Fact label="Goal">
            <Link to={paths.goal(session.goal_id)} className="block truncate hover:underline">
              {goal.data?.title ?? <Mono>{session.goal_id}</Mono>}
            </Link>
          </Fact>
        )}
        {context === "task" ? null : (
          <Fact label="Task">
            {session.task_id ? (
              <Link to={taskTo} className="block truncate hover:underline">
                {task.data?.title ?? <Mono>{session.task_id}</Mono>}
              </Link>
            ) : (
              <span className="text-muted-foreground">— (planner session)</span>
            )}
          </Fact>
        )}
        {/* One fact, not two: what the agent runs on is the tail of this line
            (`claude_code:claude-opus-5`), and a Model row under it repeated
            that tail with the CLI half taken off. The session's own snapshot,
            not the profile's current fields — the profile may have been edited
            since this agent was launched — and a session keeps the CLI and the
            model apart, so the id every other mention carries is composed
            here. */}
        <Fact label="Profile">
          <ProfileSummary
            profileId={session.profile_id}
            model={formatModelRef(session.agent_kind, session.model)}
          />
        </Fact>
        <Fact label="tmux session">
          <CopyableId value={session.tmux_session} label="tmux session" className="text-xs" />
        </Fact>
        <Fact label="Worktree">
          {session.worktree_path ? (
            <CopyableId value={session.worktree_path} label="worktree path" className="text-xs" />
          ) : (
            <Dash />
          )}
        </Fact>
        <Fact label="Agent session id">
          {session.internal_session_id ? (
            <CopyableId
              value={session.internal_session_id}
              label="agent session id"
              className="text-xs"
            />
          ) : (
            <Dash />
          )}
        </Fact>
        <Fact label="Review round">{session.review_round ?? <Dash />}</Fact>
        {/* Every transcript this agent reported under, summed — so a session
            resumed into the same agent conversation reads as one figure. Zeros
            until it reports anything, which is a number and not a blank: an
            agent that has spent nothing is what a session just spawned is. */}
        <Fact label="Tokens">
          <TokenFigure usage={session.usage} />
        </Fact>
        {session.attention_reason ? (
          <Fact label="Needs attention since">
            <When at={session.attention_since} label="since" />
          </Fact>
        ) : null}
        <Fact label="Started">
          <When at={session.created_at} label="started" />
        </Fact>
        <Fact label={session.ended_at ? "Ended" : "Last activity"}>
          <When
            at={session.ended_at ?? session.last_activity_at}
            label={session.ended_at ? "ended" : "last activity"}
          />
        </Fact>
      </FactList>

      <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
        <TabsList>
          <TabsTrigger value="terminal">Terminal</TabsTrigger>
          <TabsTrigger value="activity">Agent activity</TabsTrigger>
        </TabsList>
        <TabsContent value="terminal" className="pt-3">
          <SessionTerminal
            sessionId={session.id}
            status={session.status}
            autoFocus={focusTerminal}
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
